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

RUN cargo build --release --package customer-service

COPY services/customer-service/src services/customer-service/src
COPY services/customer-service/migrations services/customer-service/migrations

RUN touch services/customer-service/src/main.rs

RUN cargo build --release --package customer-service

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