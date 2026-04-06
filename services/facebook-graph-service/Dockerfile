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

RUN cargo chef cook --release --package facebook-graph-service --recipe-path recipe.json

COPY services/facebook-graph-service/src services/facebook-graph-service/src
COPY services/facebook-graph-service/migrations services/facebook-graph-service/migrations

RUN cargo build --release --package facebook-graph-service

FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/facebook-graph-service /usr/local/bin/

COPY services/facebook-graph-service/migrations ./migrations

COPY services/facebook-graph-service/.env.example .env

EXPOSE 3003

CMD ["facebook-graph-service"]
