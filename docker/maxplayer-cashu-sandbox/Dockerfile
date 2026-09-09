# syntax=docker/dockerfile:1
#
# The Cashu/CDK specialist job sandbox.
#
# It is the ordinary maxplayer sandbox image plus three things a Cashu/CDK job cannot get at run
# time without paying for them: a `cdk-mintd` built at the SAME CDK version this workspace pins,
# the Cashu spec + CDK source corpus on disk at pinned commits, and a start/stop/reset rig for a
# loopback-only fakewallet test mint.
#
# The mint this image carries is a FAKEWALLET mint. It issues worthless test ecash, it binds
# 127.0.0.1 only, and its database lives in the job workdir and dies with the container. It must
# never appear in a seller's `accepted_mints` — that set is the seller's real-money surface and is
# checked twice in crates/maxplayer-core/src/payment_wallet.rs (realized mint, and the NUT-18
# payload mint). Nothing in this image writes to a seller config.
#
# Build from the REPO ROOT, after the base image exists:
#   docker buildx build -f docker/maxplayer-sandbox/Dockerfile -t maxplayer-sandbox:v0.5.8-local .
#   docker buildx build -f docker/maxplayer-cashu-sandbox/Dockerfile \
#       --build-arg BASE_IMAGE=maxplayer-sandbox:v0.5.8-local \
#       -t maxplayer-cashu-sandbox:v0.5.8-local .

ARG BASE_IMAGE=maxplayer-sandbox:v0.5.8-local

# CDK version is pinned to the version THIS workspace depends on
# (crates/maxplayer-core/Cargo.toml:59,80,81 and crates/maxplayer/Cargo.toml:105-107 — all =0.17.2).
# A test mint on a different CDK than the wallet under test would make a protocol mismatch look
# like a bug in our code.
ARG CDK_VERSION=0.17.2
# cashubtc/cdk v0.17.2 tag object.
ARG CDK_COMMIT=6132607495ae0741e412a63f2acc34e4ccddfc55
# cashubtc/nuts main as of 2026-08-23T17:08:00Z. A corpus is stale from the moment it is baked;
# the seat's MEMORY.md states this pin so the agent never quotes an old NUT as current.
ARG NUTS_COMMIT=49a909ce4d0739824b3859d4b3da21e6c1abdaeb

# ---------------------------------------------------------------------------------------------
# mintd + a seed generator
# ---------------------------------------------------------------------------------------------
FROM rust:1-bookworm AS mint-builder
ARG CDK_VERSION

# protoc is a BUILD-TIME requirement even with --no-default-features: cdk-mintd depends on
# cdk-signatory unconditionally (the local in-process signatory), and cdk-signatory 0.17.2's
# build.rs compiles src/proto/signatory.proto whether or not the `grpc` feature is on — it panics
# with "Could not find `protoc`" otherwise. Measured, not assumed: the first build of this image
# failed exactly there. protoc lives in this builder stage only; it is not in the final image.
RUN apt-get update \
    && apt-get install -y --no-install-recommends protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

# --no-default-features drops cln, lnd, lnbits, bdk, ldk-node, grpc-processor and the management
# RPC. What is left is exactly the isolated testing mode: `fakewallet` (= dep:cdk-fake-wallet) and
# the sqlite store. cdk-mintd's own config comment is explicit that fakewallet "is isolated testing
# mode and cannot be mixed with real payment backends" — building without the real backends means
# this binary CANNOT be pointed at one, which is a stronger guarantee than a config convention.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,id=cashu-mint-target,target=/tmp/target \
    set -eux; \
    CARGO_TARGET_DIR=/tmp/target cargo install cdk-mintd \
        --version "${CDK_VERSION}" --locked \
        --no-default-features --features fakewallet,sqlite \
        --root /out; \
    strip /out/bin/cdk-mintd

# cdk-mintd REQUIRES a seed: lib.rs:1238 bails with "No seed nor remote signatory set", and
# --seed-file is parsed as a BIP-39 mnemonic (lib.rs:276-286). There is no auto-generation. So the
# rig mints a throwaway mnemonic per reset, into a 0600 file in the job workdir, and passes it by
# --seed-file — never on a command line, never in a log, never in this image. bip39 2.2.2 is the
# version cdk-mintd 0.17.2 already resolves, so it comes out of the same warm registry.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,id=cashu-mint-target,target=/tmp/target \
    set -eux; \
    mkdir -p /seedgen/src; \
    printf '%s\n' \
        '[package]' \
        'name = "test-mint-seed"' \
        'version = "0.1.0"' \
        'edition = "2021"' \
        '' \
        '[dependencies]' \
        'bip39 = { version = "2.2.2", features = ["rand"] }' \
        > /seedgen/Cargo.toml; \
    printf '%s\n' \
        'fn main() {' \
        '    let m = bip39::Mnemonic::generate(12).expect("generate mnemonic");' \
        '    println!("{m}");' \
        '}' \
        > /seedgen/src/main.rs; \
    CARGO_TARGET_DIR=/tmp/target cargo install --path /seedgen --root /out; \
    strip /out/bin/test-mint-seed

# The acceptance harness. It is baked rather than compiled at run time so the gate maxie runs is a
# single command with no build step and no network fetch, and so the binary is pinned to the same
# CDK as the mint and the seller.
COPY docker/maxplayer-cashu-sandbox/acceptance /acceptance
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,id=cashu-mint-target,target=/tmp/target \
    set -eux; \
    CARGO_TARGET_DIR=/tmp/target cargo install --path /acceptance --root /out; \
    strip /out/bin/mint-acceptance

# ---------------------------------------------------------------------------------------------
# knowledge corpus, at pinned commits
# ---------------------------------------------------------------------------------------------
FROM debian:bookworm-slim AS corpus
ARG CDK_COMMIT
ARG NUTS_COMMIT
RUN apt-get update \
    && apt-get install -y --no-install-recommends git ca-certificates \
    && rm -rf /var/lib/apt/lists/*
# --filter=blob:none keeps the clone small; the checkout still materialises every file at the pin.
# .git is removed afterwards so the corpus cannot be silently updated inside a job, and so the
# image does not carry history it will never read.
RUN set -eux; \
    mkdir -p /corpus; \
    git clone --filter=blob:none --no-checkout https://github.com/cashubtc/nuts /corpus/nuts; \
    git -C /corpus/nuts checkout -q "${NUTS_COMMIT}"; \
    rm -rf /corpus/nuts/.git; \
    git clone --filter=blob:none --no-checkout https://github.com/cashubtc/cdk /corpus/cdk; \
    git -C /corpus/cdk checkout -q "${CDK_COMMIT}"; \
    rm -rf /corpus/cdk/.git; \
    printf '%s\n' \
        "cashubtc/nuts   ${NUTS_COMMIT}" \
        "cashubtc/cdk    ${CDK_COMMIT}  (tag v0.17.2)" \
        > /corpus/PINS.txt

# ---------------------------------------------------------------------------------------------
# the specialist image
# ---------------------------------------------------------------------------------------------
FROM ${BASE_IMAGE}
ARG CDK_VERSION
ARG CDK_COMMIT
ARG NUTS_COMMIT

COPY --from=mint-builder /out/bin/cdk-mintd /usr/local/bin/cdk-mintd
COPY --from=mint-builder /out/bin/test-mint-seed /usr/local/bin/test-mint-seed
COPY --from=mint-builder /out/bin/mint-acceptance /usr/local/bin/mint-acceptance
COPY --from=corpus /corpus /opt/cashu/corpus
COPY docker/maxplayer-cashu-sandbox/test-mint.config.toml /opt/cashu/test-mint.config.toml
COPY docker/maxplayer-cashu-sandbox/bin/test-mint /usr/local/bin/test-mint

# The job runs as a host uid with no passwd entry, so everything it must read has to be
# world-readable and everything it must run world-executable (same rule as the base image's
# `install -m 0755`).
RUN set -eux; \
    chmod 0755 /usr/local/bin/cdk-mintd /usr/local/bin/test-mint-seed /usr/local/bin/test-mint \
        /usr/local/bin/mint-acceptance; \
    chmod -R a+rX /opt/cashu; \
    cdk-mintd --version; \
    test-mint --help >/dev/null

LABEL ai.maxplayer.specialist="cashu-cdk" \
      ai.maxplayer.cashu.cdk-version="${CDK_VERSION}" \
      ai.maxplayer.cashu.cdk-commit="${CDK_COMMIT}" \
      ai.maxplayer.cashu.nuts-commit="${NUTS_COMMIT}" \
      ai.maxplayer.cashu.mint-backend="fakewallet (worthless test ecash, loopback only)"
