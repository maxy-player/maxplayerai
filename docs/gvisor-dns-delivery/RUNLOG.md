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

## Gate status

| Gate | State |
| --- | --- |
| 1 repro + runc control, digests, causal evidence | in progress |
| 2 DNS + TLS from real shared job namespace, fresh + recreated | not started |
| 3 doctor/readiness on the real sandbox route + regression tests | not started |
| 4 real container-side Git delivery, remote hash match | not started |
| 5 private/metadata denial + concurrent public success | not started |
| 6 bounded gate script, executed test counts, PR | not started |
