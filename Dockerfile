# syntax=docker/dockerfile:1
FROM rust:1.88-trixie AS builder
WORKDIR /workspace

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY packages/hangar-cli/Cargo.toml packages/hangar-cli/Cargo.toml
COPY packages/hangar-mcp/Cargo.toml packages/hangar-mcp/Cargo.toml
COPY packages/hangar-server/Cargo.toml packages/hangar-server/Cargo.toml
COPY packages/hangar-server/build.rs packages/hangar-server/build.rs
COPY packages/hangar-server/proto packages/hangar-server/proto
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/workspace/target,sharing=locked \
    mkdir -p packages/hangar-cli/src packages/hangar-mcp/src packages/hangar-server/src \
    && printf 'fn main() {}\n' > packages/hangar-cli/src/main.rs \
    && printf 'fn main() {}\n' > packages/hangar-mcp/src/main.rs \
    && printf 'fn main() {}\n' > packages/hangar-server/src/main.rs \
    && cargo build --release --locked

COPY packages packages
# Docker preserves source timestamps; touch prevents the dependency-cache
# placeholder source from being considered newer than the real application.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/workspace/target,sharing=locked \
    touch packages/hangar-cli/src/main.rs packages/hangar-mcp/src/main.rs packages/hangar-server/src/main.rs \
    && cargo fmt --all -- --check \
    && cargo test --workspace --locked \
    && cargo build --release --locked \
    && cp target/release/hangar-server /workspace/hangar-server \
    && cp target/release/hangar-cli /workspace/hangar \
    && cp target/release/hangar-mcp /workspace/hangar-mcp

FROM debian:trixie-slim
RUN useradd --system --create-home --uid 10001 hangar
COPY --from=builder /workspace/hangar-server /usr/local/bin/hangar-server
COPY --from=builder /workspace/hangar /usr/local/bin/hangar
COPY --from=builder /workspace/hangar-mcp /usr/local/bin/hangar-mcp
VOLUME ["/var/lib/hangar"]
EXPOSE 8080
ENV HANGAR_DATA_DIR=/var/lib/hangar
ENV HANGAR_LISTEN_ADDR=0.0.0.0:8080
ENV HANGAR_GRPC_LISTEN_ADDR=0.0.0.0:50051
# Docker volumes are created as root. Prepare only the mounted data directory,
# then drop privileges before the server accesses user data.
ENTRYPOINT ["sh", "-ec", "mkdir -p \"$HANGAR_DATA_DIR\" && chown -R hangar:hangar \"$HANGAR_DATA_DIR\" && exec runuser -u hangar -- hangar-server"]
