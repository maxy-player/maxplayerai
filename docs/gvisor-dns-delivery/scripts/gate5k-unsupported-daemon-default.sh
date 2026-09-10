#!/usr/bin/env bash
# gate5k — does an UNSUPPORTED daemon default runtime fail CLOSED?
#
# Maxie's ruling (9 Sep 2026): "Missing --runtime means daemon default, not
# guaranteed runc; test unsupported defaults fail closed."
#
# That correction lands. `holder_argv()` deliberately passes NO `--runtime`, and
# a test (`the_containment_plane_never_carries_the_jobs_runtime`) locks it there,
# because a runsc holder's namespace is unusable: a job joining it sees `lo` only
# (gate2a). The design therefore ASSUMES the daemon default is runc, and nothing
# in the product verifies that assumption. This gate measures what actually
# happens when the assumption is false.
#
# Fail CLOSED  = the job gets no usable egress (or never starts).
# Fail OPEN    = the job runs and reaches a LIVE listener with no containment.
# Only the second is a security defect; the first is an availability failure.
#
# Discipline carried from gate5c/5d/5f: ONE gVisor container per namespace, a
# fresh namespace per probe, a LIVE listener as the target, and a health check
# printed beside every leg so a timeout is never read as containment.
#
# Runs inside the disposable gvisor-repro VM ONLY. It edits
# /etc/docker/daemon.json and restarts docker, so it must never be pointed at a
# shared or production daemon.
set -uo pipefail

IMAGE="${IMAGE:-ghcr.io/makeprisms/maxplayer-job:v0.5.8}"
DAEMON_JSON=/etc/docker/daemon.json
BACKUP="/home/forge.guest/daemon.json.gate5k.bak"
NET="gate5k-net"
FAIL=0

say() { printf '%s\n' "$*"; }
hr()  { printf -- '---- %s\n' "$*"; }

# ---------------------------------------------------------------- guard rails
if [ ! -f "${DAEMON_JSON}" ]; then say "ABORT: no ${DAEMON_JSON}"; exit 2; fi
if ! command -v limactl >/dev/null 2>&1; then :; else
  say "ABORT: limactl present — this looks like the HOST, not the disposable VM"; exit 2
fi

default_runtime() { sudo docker info --format '{{.DefaultRuntime}}' 2>/dev/null; }

restore() {
  hr "restoring ${DAEMON_JSON}"
  if [ -f "${BACKUP}" ]; then
    sudo cp "${BACKUP}" "${DAEMON_JSON}"
    sudo systemctl restart docker || true
    sleep 4
    say "restored default runtime = $(default_runtime)"
  fi
  sudo docker rm -f gate5k-listener-holder gate5k-listener gate5k-ns1 gate5k-ns2 >/dev/null 2>&1
  sudo docker network rm "${NET}" >/dev/null 2>&1
}
trap restore EXIT

sudo cp "${DAEMON_JSON}" "${BACKUP}"
say "gate5k — unsupported daemon default runtime"
say "kernel:  $(uname -srm)"
say "docker:  $(sudo docker version --format '{{.Server.Version}}')"
say "runsc:   $(runsc --version 2>/dev/null | head -1)"
say "ORIGINAL default runtime: $(default_runtime)"
say ""

sudo docker network create --driver bridge "${NET}" >/dev/null 2>&1

# A LIVE listener, so "denied" is distinguishable from "nothing was there".
# It runs under runsc: gate5g proved a runc listener's own OUTPUT rules can drop
# its replies and confound the read.
sudo timeout 120 docker run --detach --name gate5k-listener-holder \
  --runtime runsc --network "${NET}" "${IMAGE}" sleep 600 >/dev/null 2>&1
sleep 3
LISTEN_ADDR="$(sudo docker inspect -f "{{(index .NetworkSettings.Networks \"${NET}\").IPAddress}}" gate5k-listener-holder 2>/dev/null)"
say "listener address: ${LISTEN_ADDR:-<none>}"

# A daemon restart stops these containers (restart policy "no"), so the listener
# must be revived and RE-CHECKED before every leg. gate5g's confounded first run
# is the reason: a leg whose target was never serving proves nothing at all.
serve() {
  sudo docker start gate5k-listener-holder >/dev/null 2>&1
  sleep 3
  sudo docker exec --detach gate5k-listener-holder \
    sh -c 'while true; do printf "HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nALIVE" | nc -l -p 8080 -q 1; done' >/dev/null 2>&1
  sleep 3
}

liveness() { # prints SERVING or NOT-SERVING
  local got
  got="$(sudo timeout 60 docker run --rm --runtime runc --network "${NET}" "${IMAGE}" \
    sh -c "wget -q -T 5 -O - http://${LISTEN_ADDR}:8080/ 2>/dev/null" 2>/dev/null)"
  if [ "${got}" = "ALIVE" ]; then echo "SERVING"; else echo "NOT-SERVING"; fi
}
say ""

# probe: launch a product-SHAPED holder carrying NO --runtime (exactly what
# holder_argv does), then run the job in that holder's namespace under runsc.
probe() { # ns_name
  local ns="$1"
  sudo docker rm -f "${ns}" >/dev/null 2>&1
  sudo timeout 120 docker run --detach --name "${ns}" --network "${NET}" \
    "${IMAGE}" sleep 600 >/dev/null 2>&1
  local started=$?
  sleep 3
  if ! sudo docker ps --format '{{.Names}}' | grep -qx "${ns}"; then
    say "  holder: DID NOT START (started=${started}) -> job cannot run at all"
    return 3
  fi
  say "  holder runtime: $(sudo docker inspect -f '{{.HostConfig.Runtime}}' "${ns}" 2>/dev/null)"
  say "  holder ifaces:  $(sudo docker exec "${ns}" ip -o link 2>/dev/null | awk -F': ' '{printf "%s ", $2}')"
  # the job: ONE gVisor container in this namespace, never reused
  sudo timeout 120 docker run --rm --runtime runsc --network "container:${ns}" "${IMAGE}" \
    sh -c "ip -o addr 2>/dev/null | awk '{printf \"%s(%s) \", \$2, \$4}'; echo; \
           wget -q -T 8 -O - http://${LISTEN_ADDR}:8080/ 2>/dev/null && echo ' <= REACHED' || echo ' <= no-answer'" 2>&1 \
    | sed 's/^/  job: /'
}

hr "LEG 1 — control: daemon default is runc (the supported configuration)"
say "default runtime now: $(default_runtime)"
serve
L1="$(liveness)"
say "listener liveness: ${L1}"
[ "${L1}" = "SERVING" ] || FAIL=$((FAIL + 1))
probe gate5k-ns1
say ""

hr "LEG 2 — daemon default switched to runsc (the UNSUPPORTED configuration)"
printf '{\n  "default-runtime": "runsc",\n  "runtimes": { "runsc": { "path": "/usr/local/bin/runsc" } }\n}\n' \
  | sudo tee "${DAEMON_JSON}" >/dev/null
sudo systemctl restart docker
sleep 6
NOW="$(default_runtime)"
say "default runtime now: ${NOW}"
if [ "${NOW}" != "runsc" ]; then
  say "could not switch the default runtime — leg 2 is NO EVIDENCE"
  FAIL=$((FAIL + 1))
else
  serve
  L2="$(liveness)"
  say "listener liveness: ${L2}"
  if [ "${L2}" != "SERVING" ]; then
    say "listener down after the daemon restart — leg 2 is NO EVIDENCE"
    FAIL=$((FAIL + 1))
  fi
  probe gate5k-ns2
fi
say ""

hr "VERDICT"
say "Read leg 2's job line: a job that shows only 'lo' and cannot reach the live"
say "listener FAILED CLOSED (no egress, no containment bypass). A job that prints"
say "REACHED under the unsupported default FAILED OPEN and is a security defect."
say "failing_checks=${FAIL}"
exit 0
