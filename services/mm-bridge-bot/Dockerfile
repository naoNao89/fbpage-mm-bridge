FROM lukemathwalker/cargo-chef:latest-rust-1 AS planner

WORKDIR /app

COPY Cargo.toml Cargo.lock* ./
COPY services/customer-service/Cargo.toml services/customer-service/
COPY services/message-service/Cargo.toml services/message-service/
COPY services/facebook-graph-service/Cargo.toml services/facebook-graph-service/
COPY services/mm-bridge-bot/Cargo.toml services/mm-bridge-bot/

COPY services/customer-service/src services/customer-service/src
COPY services/message-service/src services/message-service/src
COPY services/facebook-graph-service/src services/facebook-graph-service/src
COPY services/mm-bridge-bot/src services/mm-bridge-bot/src

RUN cargo chef prepare --recipe-path recipe.json

FROM lukemathwalker/cargo-chef:latest-rust-1 AS builder

WORKDIR /app

COPY --from=planner /app/recipe.json recipe.json

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

RUN cargo chef cook --release --package mm-bridge-bot --recipe-path recipe.json

COPY services/mm-bridge-bot/src services/mm-bridge-bot/src

RUN cargo build --release --package mm-bridge-bot

FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/mm-bridge-bot /usr/local/bin/

CMD ["mm-bridge-bot"]
