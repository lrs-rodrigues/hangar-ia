# syntax=docker/dockerfile:1
FROM rust:1.88-bookworm AS builder
WORKDIR /workspace

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY packages/hangar-server/Cargo.toml packages/hangar-server/Cargo.toml
RUN mkdir -p packages/hangar-server/src && printf 'fn main() {}\n' > packages/hangar-server/src/main.rs && cargo build --release --locked

COPY packages/hangar-server packages/hangar-server
# Docker preserves source timestamps; touch prevents the dependency-cache
# placeholder source from being considered newer than the real application.
RUN touch packages/hangar-server/src/main.rs && cargo test --workspace --locked && cargo build --release --locked

FROM debian:bookworm-slim
RUN useradd --system --create-home --uid 10001 hangar
COPY --from=builder /workspace/target/release/hangar-server /usr/local/bin/hangar-server
VOLUME ["/var/lib/hangar"]
EXPOSE 8080
ENV HANGAR_DATA_DIR=/var/lib/hangar
ENV HANGAR_LISTEN_ADDR=0.0.0.0:8080
# Docker volumes are created as root. Prepare only the mounted data directory,
# then drop privileges before the server accesses user data.
ENTRYPOINT ["sh", "-ec", "mkdir -p \"$HANGAR_DATA_DIR\" && chown -R hangar:hangar \"$HANGAR_DATA_DIR\" && exec runuser -u hangar -- hangar-server"]
