FROM rust:1.88-slim-bookworm as builder

WORKDIR /app

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock* ./
COPY services/customer-service/Cargo.toml services/customer-service/
COPY services/message-service/Cargo.toml services/message-service/
COPY services/facebook-graph-service/Cargo.toml services/facebook-graph-service/

RUN mkdir -p src && echo "fn main() {}" > src/main.rs
RUN mkdir -p services/customer-service/src && echo 'fn main() {}' > services/customer-service/src/main.rs
RUN mkdir -p services/message-service/src && echo 'fn main() {}' > services/message-service/src/main.rs
RUN mkdir -p services/facebook-graph-service/src && echo 'fn main() {}' > services/facebook-graph-service/src/main.rs

RUN cargo build --release --package facebook-graph-service && rm -rf src services/*/src

COPY services/facebook-graph-service/migrations services/facebook-graph-service/migrations
COPY services/facebook-graph-service/src services/facebook-graph-service/src

RUN mkdir -p src services/facebook-graph-service/src && touch src/main.rs services/facebook-graph-service/src/main.rs && cargo build --release --package facebook-graph-service

FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/facebook-graph-service /usr/local/bin/

COPY services/facebook-graph-service/migrations ./migrations

COPY .env.example .env

EXPOSE 3003

CMD ["facebook-graph-service"]
