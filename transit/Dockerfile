# syntax=docker/dockerfile:1
FROM rust:1.92-slim AS builder
WORKDIR /workspace

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY cli/Cargo.toml ./cli/Cargo.toml
COPY cli/src ./cli/src
COPY cli/tests ./cli/tests
COPY mailbox/Cargo.toml ./mailbox/Cargo.toml
COPY mailbox/migrations ./mailbox/migrations
COPY mailbox/src ./mailbox/src
COPY transit/Cargo.toml ./transit/Cargo.toml
COPY transit/src ./transit/src

RUN cargo build --release -p blackhole-transit

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /workspace/target/release/blackhole-transit /usr/local/bin/blackhole-transit

ENV BLACKHOLE_TRANSIT_LISTEN=0.0.0.0:4001
EXPOSE 4001
ENTRYPOINT ["/usr/local/bin/blackhole-transit"]
