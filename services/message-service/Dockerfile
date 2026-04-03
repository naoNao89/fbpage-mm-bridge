FROM lukemathwalker/cargo-chef:latest-rust-1 AS planner

WORKDIR /app

COPY Cargo.toml Cargo.lock* ./
COPY services/customer-service/Cargo.toml services/customer-service/
COPY services/message-service/Cargo.toml services/message-service/
COPY services/facebook-graph-service/Cargo.toml services/facebook-graph-service/

RUN cargo chef prepare --recipe-path recipe.json

FROM lukemathwalker/cargo-chef:latest-rust-1 AS builder

WORKDIR /app

COPY --from=planner /app/recipe.json recipe.json

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

RUN cargo chef cook --release --package message-service --recipe-path recipe.json

COPY services/message-service/src services/message-service/src
COPY services/message-service/migrations services/message-service/migrations

RUN cargo build --release --package message-service

FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y ca-certificates libssl3 curl && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/message-service /app/message-service

COPY services/message-service/migrations ./migrations

RUN useradd -r -s /bin/false appuser && \
    chown -R appuser:appuser /app

USER appuser

EXPOSE 3002

ENV BIND_ADDRESS=0.0.0.0:3002
ENV LOG_LEVEL=info

CMD ["./message-service"]
