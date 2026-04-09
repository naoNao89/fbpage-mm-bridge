FROM rust:1.88-bookworm AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY services/customer-service/Cargo.toml services/customer-service/
COPY services/message-service/Cargo.toml services/message-service/
COPY services/facebook-graph-service/Cargo.toml services/facebook-graph-service/
COPY services/mm-bridge-bot/Cargo.toml services/mm-bridge-bot/

RUN mkdir -p services/customer-service/src && \
    mkdir -p services/message-service/src && \
    mkdir -p services/facebook-graph-service/src && \
    mkdir -p services/mm-bridge-bot/src && \
    echo 'fn main() {}' > services/customer-service/src/main.rs && \
    echo 'fn main() {}' > services/message-service/src/main.rs && \
    echo 'fn main() {}' > services/facebook-graph-service/src/main.rs && \
    echo 'fn main() {}' > services/mm-bridge-bot/src/main.rs

RUN cargo build --release --package facebook-graph-service

COPY services/facebook-graph-service/src services/facebook-graph-service/src
COPY services/facebook-graph-service/migrations services/facebook-graph-service/migrations

RUN touch services/facebook-graph-service/src/main.rs

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