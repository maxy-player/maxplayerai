#!/bin/zsh
# Round 9, second exec head (240240c, after the lint fix-up). Re-runs every §7 gate serially.
# Each log: line 1 = `git rev-parse HEAD`, line 2 = exec head + .rs diff to HEAD (must be empty), line 3 = the command.
set -u
cd "$(dirname "$0")/../.." || exit 2
EXEC=240240c
L=reports/w-remit-r9/logs
hdr() { git rev-parse HEAD; echo "exec head $EXEC; .rs diff to HEAD: $(git diff --stat $EXEC..HEAD -- '*.rs' | tr '\n' ' ')"; echo "\$ $1"; }
run() { # run <logname> <cmd...>
  local log=$L/$1; shift
  { hdr "$*"; "$@" 2>&1; echo "exit=$?"; } > "$log"
  echo "$(date -u +%H:%M:%SZ) done $log $(tail -1 "$log")" >> $L/exec2-progress.txt
}
: > $L/exec2-progress.txt
run exec2-core-wallet-nff.log cargo test -p maxplayer-core --features wallet --no-fail-fast
run exec2-cli-default.log     cargo test -p maxplayer --no-fail-fast
run exec2-cli-acp-wallet.log  cargo test -p maxplayer --features acp,wallet --no-fail-fast
run exec2-fmt.log             cargo fmt --all -- --check
run exec2-clippy.log          cargo clippy --workspace --all-targets --features wallet
{ echo "clippy map at $(git rev-parse HEAD) over exec2-clippy.log"; python3 reports/w-remit-r9/clippy-map.py $L/exec2-clippy.log; } > $L/exec2-clippy-map.txt 2>&1
{ echo "fmt overlap at $(git rev-parse HEAD) over exec2-fmt.log"; python3 reports/w-remit-r9/fmt-overlap.py $L/exec2-fmt.log; } > $L/exec2-fmt-overlap.txt 2>&1
echo "$(date -u +%H:%M:%SZ) STAGE1 gates 1-6 + maps done" >> $L/exec2-progress.txt
# every --exact record (44)
while IFS='|' read -r name cmd; do
  [ -z "$name" ] && continue
  { hdr "$cmd"; eval "$cmd" 2>&1; echo "exit=$?"; } > "$L/exact-$name-$EXEC.log"
  echo "$(date -u +%H:%M:%SZ) exact $name $(grep -m1 '^test result' "$L/exact-$name-$EXEC.log")" >> $L/exec2-progress.txt
done < reports/w-remit-r9/exact-records.txt
echo "$(date -u +%H:%M:%SZ) STAGE2 exact x44 done" >> $L/exec2-progress.txt
# money-path binary x3, CI's exact command (.github/workflows/ci.yml:233)
for i in 1 2 3; do
  run money-path-ci-$i-$EXEC.log cargo test -p maxplayer-core --release --no-default-features --features gateway,git-delivery,wallet,live-mints --locked
done
echo "$(date -u +%H:%M:%SZ) STAGE3 money-path x3 done — ALL DONE" >> $L/exec2-progress.txt
