# syntax=docker/dockerfile:1.7

# Stage 1: build the binary with cached cargo registry + target dirs.
FROM rust:1.92-slim-trixie AS builder
WORKDIR /app

# System deps for native-tls (reqwest) — albumify-ng pulls in openssl
# transitively via teloxide-core.
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy manifest first so the dependency layer caches across source-only edits.
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
    && cargo build --release \
    && rm -rf src target/release/deps/albumify_ng*

COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release \
    && strip target/release/albumify-ng

# Stage 2: minimal runtime image.
FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --no-create-home albumify

COPY --from=builder /app/target/release/albumify-ng /usr/local/bin/albumify-ng

USER albumify
ENV RUST_LOG=info
ENTRYPOINT ["/usr/local/bin/albumify-ng"]
