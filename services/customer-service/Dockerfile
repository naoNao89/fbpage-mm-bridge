# Build stage 1: Planner - prepare recipe.json
FROM lukemathwalker/cargo-chef:latest-rust-1 AS planner

WORKDIR /app

# Copy manifests first (for dependency resolution)
COPY Cargo.toml Cargo.lock* ./
COPY services/customer-service/Cargo.toml services/customer-service/
COPY services/message-service/Cargo.toml services/message-service/
COPY services/facebook-graph-service/Cargo.toml services/facebook-graph-service/

# Copy minimal source stubs for target detection (cargo chef prepare needs targets)
COPY services/customer-service/src services/customer-service/src
COPY services/message-service/src services/message-service/src
COPY services/facebook-graph-service/src services/facebook-graph-service/src

# Generate recipe.json (captures dependency graph)
RUN cargo chef prepare --recipe-path recipe.json

# Build stage 2: Builder - cook dependencies (cached layer!)
FROM lukemathwalker/cargo-chef:latest-rust-1 AS builder

WORKDIR /app

# Copy recipe from planner
COPY --from=planner /app/recipe.json recipe.json

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Cook dependencies - this layer is cached until Cargo.lock changes!
RUN cargo chef cook --release --package customer-service --recipe-path recipe.json

# Copy source code
COPY services/customer-service/src services/customer-service/src
COPY services/customer-service/migrations services/customer-service/migrations

# Build the actual binary
RUN cargo build --release --package customer-service

# Build stage 3: Runtime
FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y ca-certificates libssl3 curl && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/customer-service /app/customer-service

COPY services/customer-service/migrations ./migrations

RUN useradd -r -s /bin/false appuser && \
    chown -R appuser:appuser /app

USER appuser

EXPOSE 3001

ENV BIND_ADDRESS=0.0.0.0:3001
ENV LOG_LEVEL=info

CMD ["./customer-service"]
