#!/usr/bin/env bash
# gate5k — does an UNSUPPORTED daemon default runtime fail CLOSED?
#
# Maxie's ruling (9 Sep 2026): "Missing --runtime means daemon default, not
# guaranteed runc; test unsupported defaults fail closed."
#
# That correction lands on the product. `holder_argv()` deliberately passes NO
# `--runtime`, and a test (`the_containment_plane_never_carries_the_jobs_runtime`)
# locks it there, because a runsc holder's namespace is unusable: a job joining
# it sees `lo` only (gate2a). The design therefore ASSUMES the daemon default is
# runc, and nothing in the product verifies that assumption. This gate measures
# what actually happens when the assumption is false.
#
# Fail CLOSED = the job gets no usable egress, or never starts.
# Fail OPEN   = the job runs and reaches a LIVE listener with no containment.
# Only the second is a security defect; the first is an availability failure.
#
# v2. The v1 run was CONFOUNDED and is preserved as
# `evidence/gate5k-CONFOUNDED-harness-defect-*.txt`. Two harness defects, both
# mine: the listener was started with `docker exec` into a runsc holder, which
# runsc refuses, so the target was never serving in EITHER leg; and `ip` is
# absent from the sandbox image, so every interface listing read "exec failed".
# Fixed here by running the listener as a plain runc container whose MAIN
# command is the nc loop (no exec, and no second gVisor container in a shared
# namespace, which gate5d proved is single-use), and by reading interfaces from
# /proc/net/dev inside the job itself.
#
# Runs inside the disposable gvisor-repro VM ONLY: it edits
# /etc/docker/daemon.json and restarts docker. Never point it at a shared or
# production daemon.
set -uo pipefail

IMAGE="${IMAGE:-ghcr.io/makeprisms/maxplayer-sandbox:v0.5.8}"
DAEMON_JSON=/etc/docker/daemon.json
BACKUP="/home/forge.guest/daemon.json.gate5k.bak"
NET="gate5k-net"
FAIL=0
LISTEN_ADDR=""

say() { printf '%s\n' "$*"; }
hr()  { printf -- '---- %s\n' "$*"; }

if [ ! -f "${DAEMON_JSON}" ]; then say "ABORT: no ${DAEMON_JSON}"; exit 2; fi
if command -v limactl >/dev/null 2>&1; then
  say "ABORT: limactl present — this looks like the HOST, not the disposable VM"; exit 2
fi

default_runtime() { sudo docker info --format '{{.DefaultRuntime}}' 2>/dev/null; }

restore() {
  hr "restoring ${DAEMON_JSON}"
  if [ -f "${BACKUP}" ]; then
    sudo cp "${BACKUP}" "${DAEMON_JSON}"
    sudo systemctl restart docker || true
    sleep 5
    say "restored default runtime = $(default_runtime)"
  fi
  sudo docker rm -f gate5k-listener gate5k-ns1 gate5k-ns2 >/dev/null 2>&1
  sudo docker network rm "${NET}" >/dev/null 2>&1
}
trap restore EXIT

sudo cp "${DAEMON_JSON}" "${BACKUP}"
say "gate5k v2 — unsupported daemon default runtime"
say "kernel:  $(uname -srm)"
say "docker:  $(sudo docker version --format '{{.Server.Version}}')"
say "runsc:   $(runsc --version 2>/dev/null | head -1)"
say "ORIGINAL default runtime: $(default_runtime)"
say ""

sudo docker network create --driver bridge "${NET}" >/dev/null 2>&1

# The listener is test infrastructure, not the thing under test, so it is pinned
# to runc explicitly and stays on runc when the default flips underneath it.
# gate5g's confound (a runc listener's own OUTPUT rules dropping its replies)
# cannot arise here: gate5k installs no netns plan at all.
sudo docker rm -f gate5k-listener >/dev/null 2>&1
sudo timeout 120 docker run --detach --name gate5k-listener --runtime runc \
  --network "${NET}" "${IMAGE}" \
  sh -c 'while true; do printf "HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nALIVE" | nc -l -p 8080 -q 1; done' >/dev/null 2>&1

# A daemon restart stops containers (restart policy "no"), so the listener is
# revived and RE-CHECKED before every leg, and its address re-read because it
# may not come back on the same one.
serve() {
  sudo docker start gate5k-listener >/dev/null 2>&1
  sleep 4
  LISTEN_ADDR="$(sudo docker inspect -f "{{(index .NetworkSettings.Networks \"${NET}\").IPAddress}}" gate5k-listener 2>/dev/null)"
}

liveness() { # prints SERVING or NOT-SERVING
  local got
  got="$(sudo timeout 60 docker run --rm --runtime runc --network "${NET}" "${IMAGE}" \
    sh -c "wget -q -T 5 -O - http://${LISTEN_ADDR}:8080/ 2>/dev/null" 2>/dev/null)"
  if [ "${got}" = "ALIVE" ]; then echo "SERVING"; else echo "NOT-SERVING"; fi
}

# probe: a product-SHAPED holder carrying NO --runtime (exactly what holder_argv
# renders), then ONE gVisor job in that holder's namespace.
probe() { # ns_name
  local ns="$1"
  sudo docker rm -f "${ns}" >/dev/null 2>&1
  sudo timeout 120 docker run --detach --name "${ns}" --network "${NET}" \
    "${IMAGE}" sleep 600 >/dev/null 2>&1
  local started=$?
  sleep 4
  if ! sudo docker ps --format '{{.Names}}' | grep -qx "${ns}"; then
    say "  holder: DID NOT START (docker run exit=${started}) -> no job can run"
    return 3
  fi
  say "  holder runtime: $(sudo docker inspect -f '{{.HostConfig.Runtime}}' "${ns}" 2>/dev/null)"
  local out
  out="$(sudo timeout 120 docker run --rm --runtime runsc --network "container:${ns}" "${IMAGE}" \
    sh -c "printf 'ifaces='; awk -F: 'NR>2{printf \"%s \", \$1}' /proc/net/dev 2>/dev/null; \
           printf '| routes='; awk 'NR>1{printf \"%s \", \$1}' /proc/net/route 2>/dev/null; \
           printf '| target='; wget -q -T 8 -O - http://${LISTEN_ADDR}:8080/ 2>/dev/null \
             && printf 'REACHED' || printf 'no-answer'" 2>&1)"
  local rc=$?
  if [ ${rc} -ne 0 ] && [ -z "${out}" ]; then
    say "  job: DID NOT RUN (exit=${rc}) -> fail closed by refusal"
  else
    say "  job: ${out}"
  fi
}

hr "LEG 1 — control: daemon default is runc (the supported configuration)"
say "default runtime now: $(default_runtime)"
serve
L1="$(liveness)"
say "listener ${LISTEN_ADDR} liveness: ${L1}"
[ "${L1}" = "SERVING" ] || { say "leg 1 target is dead — NO EVIDENCE"; FAIL=$((FAIL + 1)); }
probe gate5k-ns1
say ""

hr "LEG 2 — daemon default switched to runsc (the UNSUPPORTED configuration)"
printf '{\n  "default-runtime": "runsc",\n  "runtimes": { "runsc": { "path": "/usr/local/bin/runsc" } }\n}\n' \
  | sudo tee "${DAEMON_JSON}" >/dev/null
sudo systemctl restart docker
sleep 7
NOW="$(default_runtime)"
say "default runtime now: ${NOW}"
if [ "${NOW}" != "runsc" ]; then
  say "could not switch the default runtime — leg 2 is NO EVIDENCE"
  FAIL=$((FAIL + 1))
else
  serve
  L2="$(liveness)"
  say "listener ${LISTEN_ADDR} liveness: ${L2}"
  if [ "${L2}" != "SERVING" ]; then
    say "leg 2 target is dead — NO EVIDENCE, not a fail-closed result"
    FAIL=$((FAIL + 1))
  fi
  probe gate5k-ns2
fi
say ""

hr "VERDICT"
say "Leg 2 with a SERVING listener: a job showing only 'lo', or refusing to run,"
say "FAILED CLOSED — no egress and no containment bypass. A job printing REACHED"
say "under the unsupported default FAILED OPEN and is a security defect."
say "failing_checks=${FAIL}  (a non-zero count means legs above proved nothing)"
exit 0
