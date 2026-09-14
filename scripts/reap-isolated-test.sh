#!/usr/bin/env bash
#
# Run the isolated container-reap regression: crates/maxplayer-core/tests/reap_isolated_linux.rs.
#
# That test runs the PRODUCTION reaper (`delivery_orchestrator::reap_other_processes`), which
# SIGKILLs every process except pid 1 and its caller. It must therefore be the only process tree in
# a fresh container. This script keeps two containers apart:
#   1. BUILD: compile the test binary inside `rust:1-bookworm`, with a named cargo volume and a named
#      target volume, so a second run is fast. The build container is full of processes (cargo,
#      rustc). Nothing is reaped there, because the test does not run there.
#   2. RUN: start a fresh `docker run --rm --init` container from the same image, mount the target
#      volume read-only, and run the test binary as the container command with `--ignored`. pid 1
#      is docker-init; the test binary is the only other process. The test checks that fact itself
#      before it reaps anything, and it refuses on a host.
#
# The test binary runs directly, NOT through `cargo test`: cargo would be a live process in the
# container, and the reaper would kill it.
#
# Usage:
#   ./scripts/reap-isolated-test.sh                             # debug profile
#   REAP_TEST_PROFILE=release ./scripts/reap-isolated-test.sh   # release profile
#
# Environment:
#   REAP_TEST_IMAGE          the image for both containers (default: rust:1-bookworm)
#   REAP_TEST_FEATURES       the feature set of the build (default: acp,gateway,git-delivery,wallet,
#                            the feature-union CI row)
#   REAP_TEST_PROFILE        debug (default) or release
#   REAP_TEST_CARGO_VOLUME   the named volume for the cargo registry (default: maxplayer-reap-test-cargo)
#   REAP_TEST_TARGET_VOLUME  the named volume for the target dir (default: maxplayer-reap-test-target)
#
# Exit status: the exit status of the test run. With `--nocapture` the test prints the observed
# /proc states before and after the reap.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${REAP_TEST_IMAGE:-rust:1-bookworm}"
FEATURES="${REAP_TEST_FEATURES:-acp,gateway,git-delivery,wallet}"
PROFILE="${REAP_TEST_PROFILE:-debug}"
CARGO_VOLUME="${REAP_TEST_CARGO_VOLUME:-maxplayer-reap-test-cargo}"
TARGET_VOLUME="${REAP_TEST_TARGET_VOLUME:-maxplayer-reap-test-target}"

die() { echo "reap-isolated-test: $*" >&2; exit 1; }

command -v docker >/dev/null 2>&1 || die "docker is required"
case "$PROFILE" in
  debug) PROFILE_FLAG="" ;;
  release) PROFILE_FLAG="--release" ;;
  *) die "REAP_TEST_PROFILE must be debug or release, not '$PROFILE'" ;;
esac

docker volume create "$CARGO_VOLUME" >/dev/null
docker volume create "$TARGET_VOLUME" >/dev/null

echo "reap-isolated-test: build the test binary in $IMAGE (features $FEATURES, profile $PROFILE)" >&2
# cargo names the executable in its JSON messages on stdout; diagnostics still render on stderr.
# shellcheck disable=SC2086  # PROFILE_FLAG is empty or one flag, on purpose.
BIN="$(docker run --rm \
  -v "$REPO_ROOT:/src" -w /src \
  -v "$CARGO_VOLUME:/usr/local/cargo/registry" \
  -v "$TARGET_VOLUME:/target" -e CARGO_TARGET_DIR=/target \
  "$IMAGE" \
  cargo test -p maxplayer-core --features "$FEATURES" --locked $PROFILE_FLAG \
    --test reap_isolated_linux --no-run --message-format=json-render-diagnostics \
  | sed -n 's|.*"executable":"\(/target/[^"]*/reap_isolated_linux-[^"]*\)".*|\1|p' | tail -n 1)"
[ -n "$BIN" ] || die "cargo did not report the reap_isolated_linux executable"
echo "reap-isolated-test: test binary $BIN" >&2

echo "reap-isolated-test: run it as the only process tree of a fresh --init container" >&2
docker run --rm --init \
  -v "$TARGET_VOLUME:/target:ro" \
  -e MAXPLAYER_REAP_TEST_ISOLATED=1 \
  "$IMAGE" \
  "$BIN" --ignored --test-threads=1 --nocapture
