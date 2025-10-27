FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release --bin perp-trader


# We do not need the Rust toolchain to run the binary!
FROM debian:trixie-slim AS runtime
WORKDIR /app

RUN apt-get update && apt-get install -y \
    libssl3 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN update-ca-certificates

COPY --from=builder /app/target/release/perp-trader /usr/local/bin
COPY config.toml /app/config.toml
COPY api-keys.json /app/api-keys.json
COPY bin/signers/signer-amd64.so /app/bin/signers/signer-amd64.so

ENTRYPOINT ["/usr/local/bin/perp-trader"]
