#!/usr/bin/env bash
# Gate 5: the DNS pinhole did not widen anything. From inside the job namespace,
# under runsc, non-root, cap-drop ALL, no-new-privileges:
#
#   (a) direct-IP denial — metadata, RFC1918, link-local, the host's own loopback,
#       and the IPv6 local/link-local forms the stack supports;
#   (b) NAME-based denial — a PUBLIC name that resolves to a denied address must
#       still be refused at CONNECT time. This is the leg that matters: a job can
#       always resolve, so the denial has to live in the network layer, not in the
#       resolver;
#   (c) concurrent success — public DNS, certificate-verified TLS and a real git
#       clone, in the SAME namespace in the SAME run, so "denied" cannot be a
#       namespace that simply has no network;
#   (d) cross-job containment — a second job's namespace, with a listener running
#       in it, unreachable from the first.
#
# Every leg prints its own PASS/FAIL and the script exits nonzero if any denial
# leaked. Bounded: every docker run carries a timeout, everything is removed on exit.
set -uo pipefail

IMAGE="${IMAGE:-ghcr.io/makeprisms/maxplayer-sandbox:v0.5.8}"
NETFILTER_IMAGE="${NETFILTER_IMAGE:-ghcr.io/makeprisms/maxplayer-netfilter:v0.5.8}"
NET="${NET:-maxplayer-dns-gate5}"
SUBNET="${SUBNET:-172.31.12.0/24}"
GATEWAY="${GATEWAY:-172.31.12.1}"
RESOLVER="${RESOLVER:-1.1.1.1}"
LOOPBACK_PORT="${LOOPBACK_PORT:-49251}"
PUBLIC_REPO="${PUBLIC_REPO:-https://github.com/octocat/Hello-World.git}"
PLAN_FILE="${PLAN_FILE:-$HOME/gate5-plan.txt}"
RESOLV_FILE="${RESOLV_FILE:-$HOME/gate5-resolv.conf}"
OUT="${OUT:-$HOME/gate5-evidence.txt}"
HOLDER_A="gate5-holder-a"
HOLDER_B="gate5-holder-b"
NEIGHBOUR="gate5-neighbour"
HOLDER_RUNTIME="${HOLDER_RUNTIME:-runc}"
JOB_RUNTIME="${JOB_RUNTIME:-runsc}"

exec > >(tee "${OUT}") 2>&1
fail=0

echo "=== gate5: environment ==="
date -u +"utc=%Y-%m-%dT%H:%M:%SZ"
echo "kernel=$(uname -r) arch=$(uname -m)"
. /etc/os-release && echo "os=${PRETTY_NAME}"
echo "docker=$(sudo docker version --format '{{.Server.Version}}')"
echo "runsc=$(runsc --version | head -1)"
echo "holder_runtime=${HOLDER_RUNTIME} job_runtime=${JOB_RUNTIME}"
echo "image_digest=$(sudo docker image inspect "${IMAGE}" --format '{{index .RepoDigests 0}}')"

cleanup() {
  sudo docker rm -f "${HOLDER_A}" "${HOLDER_B}" "${NEIGHBOUR}" >/dev/null 2>&1 || true
  sudo docker network rm "${NET}" >/dev/null 2>&1 || true
  pkill -f "gate5-loopback-listener" >/dev/null 2>&1 || true
}
trap cleanup EXIT
cleanup

cat > "${RESOLV_FILE}" <<EOF
nameserver ${RESOLVER}
options timeout:2 attempts:2
EOF
chmod 0444 "${RESOLV_FILE}"
echo "plan=${PLAN_FILE} rules=$(grep -c . "${PLAN_FILE}") sha256=$(sha256sum "${PLAN_FILE}" | cut -d' ' -f1)"

sudo docker network create --subnet "${SUBNET}" --gateway "${GATEWAY}" "${NET}" >/dev/null
echo "network=${NET} subnet=${SUBNET} gateway=${GATEWAY}"

# A listener on the VM's OWN loopback. Reaching it from a container would mean the
# sandbox's loopback is not its own — the exact confusion gVisor's netstack prevents
# and a misconfigured host would not.
setsid python3 -c "
import socket,sys
s=socket.socket(); s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1)
s.bind(('127.0.0.1',${LOOPBACK_PORT})); s.listen(8)
sys.stdout.write('gate5-loopback-listener up\n'); sys.stdout.flush()
while True:
    c,_=s.accept(); c.sendall(b'LEAKED'); c.close()
" >/dev/null 2>&1 </dev/null &
sleep 1
echo "host_loopback_listener=127.0.0.1:${LOOPBACK_PORT}"

establish() {
  local holder="$1"
  sudo timeout 120 docker run --detach --name "${holder}" --runtime "${HOLDER_RUNTIME}" \
    --network "${NET}" --read-only --cap-drop ALL --security-opt no-new-privileges \
    --user 65534:65534 --entrypoint sleep "${IMAGE}" infinity >/dev/null
  sudo timeout 120 docker run --rm --interactive --runtime "${HOLDER_RUNTIME}" \
    --network "container:${holder}" --cap-drop ALL --cap-add NET_ADMIN \
    --security-opt no-new-privileges "${NETFILTER_IMAGE}" < "${PLAN_FILE}" >/dev/null
  echo "established=${holder} sidecar_exit=$?"
}

echo
echo "=== gate5: two job namespaces, both contained ==="
establish "${HOLDER_A}"
establish "${HOLDER_B}"
B_ADDR="$(sudo docker inspect "${HOLDER_B}" --format "{{(index .NetworkSettings.Networks \"${NET}\").IPAddress}}")"
echo "job_b_address=${B_ADDR}"

# A listener inside job B's namespace: the neighbour a contained job must not reach.
sudo timeout 60 docker run --detach --name "${NEIGHBOUR}" --runtime "${JOB_RUNTIME}" \
  --network "container:${HOLDER_B}" --user 65534:65534 --cap-drop ALL \
  --security-opt no-new-privileges --entrypoint node "${IMAGE}" \
  -e "require('http').createServer((q,r)=>r.end('LEAKED')).listen(8080,'0.0.0.0')" >/dev/null
sleep 3
echo "neighbour_listening=$(sudo docker inspect -f '{{.State.Running}}' "${NEIGHBOUR}")"

# One payload, run as the job in namespace A. Denial legs first, success legs after,
# in the SAME container, so a pass cannot be a namespace with no network at all.
PAYLOAD='
const net=require("net"),dns=require("dns"),https=require("https");
const denied=[
  ["metadata (direct ip)","169.254.169.254",80],
  ["link-local (direct ip)","169.254.1.1",80],
  ["rfc1918 10/8","10.0.0.1",80],
  ["rfc1918 192.168/16","192.168.1.1",80],
  // With `node -e <script> a b c`, process.argv is [execPath, a, b, c] — the script is not
  // an argument, so the first extra argument is argv[1] and not argv[2].
  ["host loopback via 127.0.0.1",process.argv[2],Number(process.argv[3])],
  ["another job namespace",process.argv[1],8080],
  ["metadata BY NAME","169.254.169.254.nip.io",80],
  ["rfc1918 BY NAME","10.0.0.1.nip.io",80],
  ["ipv6 loopback","::1",80],
  ["ipv6 link-local","fe80::1",80],
  ["ipv6 unique-local","fc00::1",80],
];
let failures=0, pending=denied.length+1;
const done=()=>{ if(--pending===0){ console.log("denial_failures="+failures); } };
for(const [label,host,port] of denied){
  const s=net.connect({host,port,timeout:6000});
  const verdict=(how)=>{ console.log("DENY-PASS  "+label+" ("+host+":"+port+") -> "+how); s.destroy(); done(); };
  s.on("connect",()=>{ console.log("DENY-FAIL  "+label+" ("+host+":"+port+") -> REACHED"); failures++; s.destroy(); done(); });
  s.on("timeout",()=>verdict("timeout"));
  s.on("error",(e)=>verdict(e.code));
}
// Concurrent success, in this same namespace and this same run.
dns.lookup("relay.maxplayer.ai",(e,a)=>{
  if(e){ console.log("PUBLIC-FAIL dns "+e.code); failures++; return done(); }
  console.log("PUBLIC-PASS dns relay.maxplayer.ai -> "+a);
  const r=https.request({host:"relay.maxplayer.ai",port:443,path:"/",method:"HEAD",timeout:15000},(res)=>{
    if(res.socket.authorized){ console.log("PUBLIC-PASS tls "+res.statusCode+" verified "+((res.socket.getPeerCertificate()||{}).subject||{}).CN); }
    else { console.log("PUBLIC-FAIL tls chain not verified"); failures++; }
    done();
  });
  r.on("timeout",()=>{ console.log("PUBLIC-FAIL tls timeout"); failures++; done(); });
  r.on("error",(err)=>{ console.log("PUBLIC-FAIL tls "+err.code); failures++; done(); });
  r.end();
});
'

echo
echo "=== gate5: job A — denial legs and public legs, one namespace, one run ==="
JOB_OUT="$(sudo timeout 180 docker run --rm --runtime "${JOB_RUNTIME}" \
  --network "container:${HOLDER_A}" --user 65534:65534 --cap-drop ALL \
  --security-opt no-new-privileges -v "${RESOLV_FILE}:/etc/resolv.conf:ro" \
  --entrypoint node "${IMAGE}" -e "${PAYLOAD}" "${B_ADDR}" "127.0.0.1" "${LOOPBACK_PORT}" 2>&1)"
echo "${JOB_OUT}"

echo
echo "=== gate5: job A — git clone over https, same namespace ==="
sudo timeout 180 docker run --rm --runtime "${JOB_RUNTIME}" \
  --network "container:${HOLDER_A}" --user 65534:65534 --cap-drop ALL \
  --security-opt no-new-privileges -v "${RESOLV_FILE}:/etc/resolv.conf:ro" \
  --entrypoint sh "${IMAGE}" -c '
    export HOME=/tmp GIT_TERMINAL_PROMPT=0
    cd /tmp && git clone --quiet "$1" repo && cd repo && echo "PUBLIC-PASS git $(git rev-parse HEAD)"
  ' gate5 "${PUBLIC_REPO}"
GIT_EXIT=$?
echo "git_exit=${GIT_EXIT}"
[ "${GIT_EXIT}" -eq 0 ] || fail=1

echo
echo "=== gate5: verdict ==="
DENIAL_FAILURES="$(printf '%s\n' "${JOB_OUT}" | sed -n 's/^denial_failures=//p')"
echo "denial_failures=${DENIAL_FAILURES:-unreported}"
printf '%s\n' "${JOB_OUT}" | grep -c '^DENY-PASS' | sed 's/^/denials_proven=/'
if [ "${DENIAL_FAILURES:-1}" != "0" ]; then
  echo "DENIAL: FAIL — something a contained job must not reach was reachable"
  fail=1
else
  echo "DENIAL: PASS — every denied destination refused, by IP and by NAME"
fi
# Both halves asserted POSITIVELY. Absence of a failure line is not a success: the first run
# of this script crashed before it reached the public legs and the "no FAIL lines" test read
# that silence as a pass.
DNS_OK=$(printf '%s\n' "${JOB_OUT}" | grep -c '^PUBLIC-PASS dns')
TLS_OK=$(printf '%s\n' "${JOB_OUT}" | grep -c '^PUBLIC-PASS tls')
if printf '%s\n' "${JOB_OUT}" | grep -q '^PUBLIC-FAIL' || [ "${DNS_OK}" -eq 0 ] || [ "${TLS_OK}" -eq 0 ]; then
  echo "PUBLIC: FAIL — the sandbox did not demonstrate the public route it is supposed to keep \
(dns_ok=${DNS_OK} tls_ok=${TLS_OK} git_exit=${GIT_EXIT})"
  fail=1
else
  echo "PUBLIC: PASS — DNS, verified TLS and git all succeeded in the same namespace"
fi
if [ "${fail}" -eq 0 ]; then echo "GATE5: PASS"; else echo "GATE5: FAIL"; fi
exit "${fail}"
