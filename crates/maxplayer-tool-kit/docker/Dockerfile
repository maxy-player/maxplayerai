# Linux image carrying all five kit binaries.
#
# Build context is the crate directory, not the repository: this crate declares explicit
# dependency versions and no workspace membership requirement, so it compiles on its own. That
# keeps the image build honest about the crate's dependency surface — if it needed something
# from maxplayer-core, this build would fail rather than quietly pick it up.
#
# One image serves every role in the demo (vendor, holder, job container). The roles differ by
# entrypoint and, far more importantly, by what is mounted into them: the job container is given
# its own socket and its own directory, and nothing else.

FROM rust:1-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml ./
COPY src ./src
COPY fixtures ./fixtures
RUN cargo build --release && \
    strip target/release/vendor-service \
          target/release/vendor-cli \
          target/release/tool-holderd \
          target/release/holderctl \
          target/release/tool-mcp-bridge

FROM debian:bookworm-slim
# `grep` is present in the base image and is used by the demo to search a job container's own
# filesystem for the credential. Nothing else is installed.
COPY --from=build /src/target/release/vendor-service /usr/local/bin/
COPY --from=build /src/target/release/vendor-cli /usr/local/bin/
COPY --from=build /src/target/release/tool-holderd /usr/local/bin/
COPY --from=build /src/target/release/holderctl /usr/local/bin/
COPY --from=build /src/target/release/tool-mcp-bridge /usr/local/bin/
COPY fixtures/seller-tool-config.json /etc/maxplayer/seller-tool-config.json

# Short by construction: a Unix socket path is capped at SUN_LEN (108 bytes on Linux), and the
# per-job sockets live under this directory.
RUN mkdir -p /run/holder /var/lib/holder /srv/jobs

CMD ["/usr/local/bin/holderctl", "--help"]
