# gVisor named-network DNS + container delivery — run log

Lane: `w-gvisor-dns-delivery-r2` · ordering seat: maxie (requested by Bob)
Brief: `/Users/forge/forge/v2/maxie/runs/gvisor-dns-delivery-brief-20260909.md`
(3,851 B, sha256 `3b0a20feab6ae107e9ddc28cb70b30b1a73656c15fa95c72abc62edd25430812`, verified 2026-09-09)

Six acceptance gates. Nothing below is claimed as passing unless the command
output is recorded here or in `evidence/`.

## Environment reality (2026-09-09, recorded before any test)

| Fact | Value |
| --- | --- |
| Forge host | macOS 26.4.1, arm64 (Apple silicon) |
| Reported failing host (Bob) | Ubuntu 24.04, kernel 6.8.0-124, x86_64, Docker 29.8.0, runsc release-20260831.0 |
| Shared container runtime on this host | colima `default`, aarch64, Docker 29.7.2 client / 29.5.2 server — **off limits**, other lanes use it, no runsc install there |
| Disposable Linux for gates | lima VM `gvisor-repro`, Ubuntu 24.04 (noble release-20260705), aarch64, vz driver, 2 CPU / 4 GiB / 20 GiB |

**Architecture divergence is named, not papered over.** The reproduction host
available to this lane is aarch64; the reported evidence is x86_64. Any gate
result carries the arch it was executed on. If a failure mode proves x86-only,
that is reported as a limitation, not as a pass.

## Source map (read before edits)

- `crates/maxplayer-core/src/sandbox_net.rs` (1126 lines) — renders the egress
  policy. Already documents that loopback must never be denied because docker's
  embedded DNS answers at `127.0.0.11` inside the namespace, and carries a
  load-bearing test for it.
- `crates/maxplayer-core/src/sandbox_netns.rs` (939 lines) — puts the policy in
  force via holder → sidecar → job, all sharing one network namespace. Header
  asserts "name resolution is unaffected: a container joining the namespace
  still gets its own `/etc/resolv.conf` pointing at docker's embedded resolver
  on `127.0.0.11`". That assertion is exactly what the reported failure
  contradicts under runsc, so it is the first thing to test rather than trust.
- `crates/maxplayer-core/tests/sandbox_netns_live.rs` (840 lines) — existing
  live tests.

## Timeline

- 17:39 PDT — brief read, hash verified, worktree + branch created off
  `origin/main` @ `b45f865`.
- 17:41 PDT — disposable lima VM `gvisor-repro` creation started (Ubuntu 24.04
  cloud image, arm64). Shared colima VM deliberately untouched.
- 17:46 PDT — **failure reproduced** (`evidence/gate1-runsc-vs-runc-20260910T0046Z.txt`).
- 17:52 PDT — root cause isolated and a containment-preserving fix validated by
  hand before any code was written.

## Gate 1 result — reproduced (aarch64)

Exact run recorded in `evidence/gate1-runsc-vs-runc-20260910T0046Z.txt`.
Identical image, identical named bridge, identical container security settings;
only `--runtime` differs:

| Probe | Result |
| --- | --- |
| `dns.lookup(relay.maxplayer.ai)` under `--runtime runsc` | `ERR EAI_AGAIN`, exit 1 |
| same under `--runtime runc` (control) | `OK 34.225.223.145`, exit 0 |
| raw UDP datagram to `127.0.0.11:53` under runsc | **TIMEOUT — no answer at all** |

Both containers were handed the *same* `/etc/resolv.conf`
(`nameserver 127.0.0.11`, docker's embedded resolver, `ExtServers:
[host(127.0.0.53)]`). Digests: image
`sha256:1c50e46a35dfe91fcdbbba11876bff312a95567bda98d6dcb7f675c884777412`
(arm64/linux), docker 29.1.3, runsc release-20260817.0, kernel 6.8.0-134,
network subnet 172.18.0.0/16.

**Causal source.** The failure is not resolver policy, not the allowlist, and
not name-specific: a bare UDP packet to `127.0.0.11:53` gets no reply inside the
sandbox. Docker's embedded DNS on a *user-defined* network is a socket bound by
the daemon inside the container's network namespace on `127.0.0.11:<ephemeral>`,
reached through NAT rules installed in that namespace. Under runsc the sandbox
runs its own network stack and terminates loopback inside the sentry, so those
packets never reach the namespace-side rules or the daemon's socket. Under runc
the container shares the host kernel's stack, so they do. That is the whole
delta, and it explains why the shared job namespace fails identically — the
holder's namespace has exactly the same embedded resolver.

**`--dns` does not fix it** (measured): with `--dns 1.1.1.1` on a user-defined
network docker *still* writes `nameserver 127.0.0.11` and merely forwards
upstream from the daemon side, so the container still fails `EAI_AGAIN`. Any
fix that only sets docker DNS flags is theatre.

**Validated fix direction** (measured, same runsc runtime, same named network,
same `--user 65534:65534 --cap-drop ALL --security-opt no-new-privileges`):
supply the sandbox its own `/etc/resolv.conf` naming real upstream resolvers,
read-only, instead of the unreachable embedded one. Result: `lookup: OK
34.225.223.145`, `tls: 200 cert-verified`. Containment is untouched — still the
named bridge, no host networking, no runc, no added capability. The egress
policy must then explicitly permit port 53 to exactly those resolver addresses
and nothing wider.

## Gate status

| Gate | State |
| --- | --- |
| 1 repro + runc control, digests, causal evidence | **done (aarch64), evidence committed** |
| 2 DNS + TLS from real shared job namespace, fresh + recreated | not started |
| 3 doctor/readiness on the real sandbox route + regression tests | not started |
| 4 real container-side Git delivery, remote hash match | not started |
| 5 private/metadata denial + concurrent public success | not started |
| 6 bounded gate script, executed test counts, PR | not started |
