#!/usr/bin/env bash
# Gate 5c: if the netns OUTPUT chain does not bind a gVisor job, what does?
#
# gate5b established the hole: runsc jobs ignore the plan installed in their own
# network namespace, because gVisor's netstack writes frames to the veth itself and
# the host kernel's OUTPUT chain there only sees host sockets. This script measures
# the two candidate sites on the OTHER side of the veth, where the host kernel
# handles the packet no matter what produced it.
#
#   (a) DOCKER-USER (root-netns FORWARD path), keyed to the job namespace's source
#       address. Docker guarantees DOCKER-USER is consulted before its own rules.
#   (b) a per-job network instead of the single shared bridge every job sits on today.
#
# Every denial leg targets a LIVE listener. A timeout to an address where nothing
# listens is not evidence of policy — that mistake is what made part of gate 2 worthless.
set -uo pipefail

IMAGE="${IMAGE:-ghcr.io/makeprisms/maxplayer-sandbox:v0.5.8}"
NETFILTER_IMAGE="${NETFILTER_IMAGE:-ghcr.io/makeprisms/maxplayer-netfilter:v0.5.8}"
NET_A="${NET_A:-maxplayer-dns-gate5c-a}"
NET_B="${NET_B:-maxplayer-dns-gate5c-b}"
SUBNET_A="${SUBNET_A:-172.31.14.0/24}"
GATEWAY_A="${GATEWAY_A:-172.31.14.1}"
SUBNET_B="${SUBNET_B:-172.31.15.0/24}"
GATEWAY_B="${GATEWAY_B:-172.31.15.1}"
RESOLVER="${RESOLVER:-1.1.1.1}"
PUBLIC_REPO="${PUBLIC_REPO:-https://github.com/octocat/Hello-World.git}"
PLAN_FILE="${PLAN_FILE:-$HOME/gate5c-plan.txt}"
RESOLV_FILE="${RESOLV_FILE:-$HOME/gate5c-resolv.conf}"
OUT="${OUT:-$HOME/gate5c-evidence.txt}"
HOLDER_A="gate5c-holder-a"
HOLDER_SAME="gate5c-holder-same"
HOLDER_FAR="gate5c-holder-far"
NEIGH_SAME="gate5c-neighbour-same"
NEIGH_FAR="gate5c-neighbour-far"

exec > >(tee "${OUT}") 2>&1

echo "=== gate5c: environment ==="
date -u +"utc=%Y-%m-%dT%H:%M:%SZ"
echo "kernel=$(uname -r) arch=$(uname -m)"
echo "docker=$(sudo docker version --format '{{.Server.Version}}') runsc=$(runsc --version | head -1)"
echo "br_netfilter=$(cat /proc/sys/net/bridge/bridge-nf-call-iptables 2>/dev/null || echo absent)"

RULE_ADDED=""
cleanup() {
  [ -n "${RULE_ADDED}" ] && sudo iptables -D DOCKER-USER ${RULE_ADDED} >/dev/null 2>&1
  sudo docker rm -f "${HOLDER_A}" "${HOLDER_SAME}" "${HOLDER_FAR}" "${NEIGH_SAME}" "${NEIGH_FAR}" >/dev/null 2>&1 || true
  sudo docker network rm "${NET_A}" "${NET_B}" >/dev/null 2>&1 || true
}
trap cleanup EXIT
cleanup

printf 'nameserver %s\noptions timeout:2 attempts:2\n' "${RESOLVER}" > "${RESOLV_FILE}"
chmod 0444 "${RESOLV_FILE}"

sudo docker network create --subnet "${SUBNET_A}" --gateway "${GATEWAY_A}" "${NET_A}" >/dev/null
sudo docker network create --subnet "${SUBNET_B}" --gateway "${GATEWAY_B}" "${NET_B}" >/dev/null
echo "net_a=${NET_A} ${SUBNET_A} | net_b=${NET_B} ${SUBNET_B}"

holder() { # name network
  sudo timeout 120 docker run --detach --name "$1" --runtime runc --network "$2" \
    --read-only --cap-drop ALL --security-opt no-new-privileges --user 65534:65534 \
    --entrypoint sleep "${IMAGE}" infinity >/dev/null
}
addr_of() { sudo docker inspect "$1" --format "{{(index .NetworkSettings.Networks \"$2\").IPAddress}}"; }
listener() { # name holder
  sudo timeout 60 docker run --detach --name "$1" --runtime runc --network "container:$2" \
    --user 65534:65534 --cap-drop ALL --security-opt no-new-privileges \
    --entrypoint node "${IMAGE}" \
    -e "require('http').createServer((q,r)=>r.end('REACHED')).listen(8080,'0.0.0.0')" >/dev/null
}

holder "${HOLDER_A}" "${NET_A}"
holder "${HOLDER_SAME}" "${NET_A}"
holder "${HOLDER_FAR}" "${NET_B}"
sudo timeout 120 docker run --rm --interactive --runtime runc \
  --network "container:${HOLDER_A}" --cap-drop ALL --cap-add NET_ADMIN \
  --security-opt no-new-privileges "${NETFILTER_IMAGE}" < "${PLAN_FILE}" >/dev/null
listener "${NEIGH_SAME}" "${HOLDER_SAME}"
listener "${NEIGH_FAR}" "${HOLDER_FAR}"
sleep 3

A_ADDR="$(addr_of "${HOLDER_A}" "${NET_A}")"
SAME_ADDR="$(addr_of "${HOLDER_SAME}" "${NET_A}")"
FAR_ADDR="$(addr_of "${HOLDER_FAR}" "${NET_B}")"
echo "job_a=${A_ADDR} | live_neighbour_same_network=${SAME_ADDR}:8080 | live_neighbour_other_network=${FAR_ADDR}:8080"

PROBE='
const net=require("net");
const s=net.connect({host:process.argv[1],port:Number(process.argv[2]),timeout:8000});
let said=false; const say=(w)=>{ if(!said){ said=true; console.log(w); } s.destroy(); };
s.on("connect",()=>say("REACHED"));
s.on("timeout",()=>say("timeout"));
s.on("error",(e)=>say(e.code));
'
probe() { # runtime host port
  sudo timeout 60 docker run --rm --runtime "$1" --network "container:${HOLDER_A}" \
    --user 65534:65534 --cap-drop ALL --security-opt no-new-privileges \
    -v "${RESOLV_FILE}:/etc/resolv.conf:ro" --entrypoint node "${IMAGE}" \
    -e "${PROBE}" "$2" "$3" 2>&1 | tr -d '\r' | tail -1
}

echo
echo "=== gate5c: baseline — no host-side rule (this is today's behaviour) ==="
echo "runc  -> live neighbour, same bridge: $(probe runc  "${SAME_ADDR}" 8080)"
echo "runsc -> live neighbour, same bridge: $(probe runsc "${SAME_ADDR}" 8080)"

echo
echo "=== gate5c: (b) per-job network — the neighbour is on a DIFFERENT bridge ==="
echo "runsc -> live neighbour, other bridge: $(probe runsc "${FAR_ADDR}" 8080)"

echo
echo "=== gate5c: (a) DOCKER-USER, keyed to the job namespace's source address ==="
RULE_ADDED="-s ${A_ADDR}/32 -d 172.16.0.0/12 -j DROP"
sudo iptables -I DOCKER-USER ${RULE_ADDED}
echo "installed: iptables -I DOCKER-USER ${RULE_ADDED}"
echo "runc  -> live neighbour, same bridge: $(probe runc  "${SAME_ADDR}" 8080)"
echo "runsc -> live neighbour, same bridge: $(probe runsc "${SAME_ADDR}" 8080)"

echo
echo "=== gate5c: does that rule cost the public route the job must keep? ==="
sudo timeout 120 docker run --rm --runtime runsc --network "container:${HOLDER_A}" \
  --user 65534:65534 --cap-drop ALL --security-opt no-new-privileges \
  -v "${RESOLV_FILE}:/etc/resolv.conf:ro" --entrypoint node "${IMAGE}" -e '
const dns=require("dns"),https=require("https");
dns.lookup("relay.maxplayer.ai",(e,a)=>{
  if(e) return console.log("PUBLIC-FAIL dns "+e.code);
  console.log("PUBLIC-PASS dns relay.maxplayer.ai -> "+a);
  const r=https.request({host:"relay.maxplayer.ai",port:443,path:"/",method:"HEAD",timeout:15000},(res)=>{
    console.log((res.socket.authorized?"PUBLIC-PASS":"PUBLIC-FAIL")+" tls "+res.statusCode+" verified="+res.socket.authorized);
    res.resume(); r.destroy();
  });
  r.on("timeout",()=>{console.log("PUBLIC-FAIL tls timeout");r.destroy();});
  r.on("error",(x)=>console.log("PUBLIC-FAIL tls "+x.code));
  r.end();
});
' 2>&1 | tr -d '\r'

sudo timeout 180 docker run --rm --runtime runsc --network "container:${HOLDER_A}" \
  --user 65534:65534 --cap-drop ALL --security-opt no-new-privileges \
  -v "${RESOLV_FILE}:/etc/resolv.conf:ro" --entrypoint sh "${IMAGE}" -c '
    export HOME=/tmp GIT_TERMINAL_PROMPT=0
    cd /tmp && git clone --quiet "$1" repo 2>&1 && echo "PUBLIC-PASS git $(git -C repo rev-parse HEAD)"
  ' gate5c "${PUBLIC_REPO}" 2>&1 | tr -d '\r'

echo
echo "=== gate5c: read the finding off the table above ==="
echo "(a) binds runsc if the DOCKER-USER runs show 'timeout' for BOTH runtimes"
echo "(b) binds runsc if the other-bridge probe is not REACHED"
