# syntax=docker/dockerfile:1.7

FROM rust:1.95-slim-bookworm AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release \
    && rm -rf src target/release/deps/poker_blind_timer_server*

COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/poker-blind-timer-server /usr/local/bin/poker-blind-timer-server

RUN mkdir -p /data

ENV PORT=8080
EXPOSE 8080
CMD ["/usr/local/bin/poker-blind-timer-server"]
