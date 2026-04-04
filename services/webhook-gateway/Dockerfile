# Build stage 1: Planner - prepare recipe.json
FROM lukemathwalker/cargo-chef:latest-rust-1 AS planner

WORKDIR /app

COPY Cargo.toml Cargo.lock* ./
COPY services/customer-service/Cargo.toml services/customer-service/
COPY services/message-service/Cargo.toml services/message-service/
COPY services/facebook-graph-service/Cargo.toml services/facebook-graph-service/
COPY services/webhook-gateway/Cargo.toml services/webhook-gateway/

COPY services/customer-service/src services/customer-service/src
COPY services/message-service/src services/message-service/src
COPY services/facebook-graph-service/src services/facebook-graph-service/src
COPY services/webhook-gateway/src services/webhook-gateway/src

RUN cargo chef prepare --recipe-path recipe.json

# Build stage 2: Builder - cook dependencies (cached layer!)
FROM lukemathwalker/cargo-chef:latest-rust-1 AS builder

WORKDIR /app

COPY --from=planner /app/recipe.json recipe.json

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

RUN cargo chef cook --release --package webhook-gateway --recipe-path recipe.json

COPY services/webhook-gateway/src services/webhook-gateway/src

RUN cargo build --release --package webhook-gateway

# Build stage 3: Runtime
FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y ca-certificates libssl3 curl && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/webhook-gateway /app/webhook-gateway

RUN useradd -r -s /bin/false appuser && \
    chown -R appuser:appuser /app

USER appuser

EXPOSE 3004

ENV BIND_ADDRESS=0.0.0.0:3004
ENV LOG_LEVEL=info

CMD ["./webhook-gateway"]
