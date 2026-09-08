#!/bin/zsh
# Round 10 (addendum 11) §4 I gates, SECOND executable head (after the clippy identity_op fix to record 45's test literal).
# Usage: zsh reports/w-remit-r10/run-gates-r10b.sh <exec-head-sha>   — same gates as run-gates-r10.sh, same record list.
# Raw logs live OUTSIDE the tree in /tmp/w-remit-r10/logs-b/ and are copied (cp -p + cmp) to the seat evidence folder
# /Users/forge/forge/v2/worker/reports/w-remit-r10/logs-b/ per the evidence-location ruling; nothing >200 lines under reports/.
# Each log: line 1 = `git rev-parse HEAD`, line 2 = exec head + .rs diff to HEAD (must be empty), line 3 = the command.
set -u
cd "$(dirname "$0")/../.." || exit 2
export PATH="$HOME/.cargo/bin:$PATH"
EXEC=${1:?exec head sha required}
L=/tmp/w-remit-r10/logs-b
mkdir -p $L
hdr() { git rev-parse HEAD; echo "exec head $EXEC; .rs diff to HEAD: $(git diff --stat $EXEC..HEAD -- '*.rs' | tr '\n' ' ')"; echo "\$ $1"; }
run() { # run <logname> <cmd...>
  local log=$L/$1; shift
  { hdr "$*"; "$@" 2>&1; echo "exit=$?"; } > "$log"
  echo "$(date -u +%H:%M:%SZ) done $log $(tail -1 "$log")" >> $L/progress.txt
}
: > $L/progress.txt
echo "$(date -u +%H:%M:%SZ) runner b start; exec head $EXEC; HEAD $(git rev-parse HEAD)" >> $L/progress.txt
run core-wallet-nff-$EXEC.log cargo test -p maxplayer-core --features wallet --no-fail-fast
run cli-default-$EXEC.log     cargo test -p maxplayer --no-fail-fast
run cli-acp-wallet-$EXEC.log  cargo test -p maxplayer --features acp,wallet --no-fail-fast
run fmt-$EXEC.log             cargo fmt --all -- --check
run clippy-$EXEC.log          cargo clippy --workspace --all-targets --features wallet
{ echo "clippy map at $(git rev-parse HEAD) over clippy-$EXEC.log"; python3 reports/w-remit-r9/clippy-map.py $L/clippy-$EXEC.log; } > $L/clippy-map-$EXEC.txt 2>&1
{ echo "fmt overlap at $(git rev-parse HEAD) over fmt-$EXEC.log"; python3 reports/w-remit-r9/fmt-overlap.py $L/fmt-$EXEC.log; } > $L/fmt-overlap-$EXEC.txt 2>&1
echo "$(date -u +%H:%M:%SZ) STAGE1 gates 1-5 + maps done" >> $L/progress.txt
# every --exact record (44 + 4 new = 48 lines; #33 retired)
while IFS='|' read -r name cmd; do
  [ -z "$name" ] && continue
  case "$name" in \#*) continue;; esac
  { hdr "$cmd"; eval "$cmd" 2>&1; echo "exit=$?"; } > "$L/exact-$name-$EXEC.log"
  echo "$(date -u +%H:%M:%SZ) exact $name $(grep -m1 '^test result' "$L/exact-$name-$EXEC.log")" >> $L/progress.txt
done < reports/w-remit-r10/exact-records-r10.txt
echo "$(date -u +%H:%M:%SZ) STAGE2 exact x47 done" >> $L/progress.txt
# money-path binary x3, CI's exact command (.github/workflows/ci.yml:233)
for i in 1 2 3; do
  run money-path-ci-$i-$EXEC.log cargo test -p maxplayer-core --release --no-default-features --features gateway,git-delivery,wallet,live-mints --locked
done
echo "$(date -u +%H:%M:%SZ) STAGE3 money-path x3 done — ALL DONE" >> $L/progress.txt
