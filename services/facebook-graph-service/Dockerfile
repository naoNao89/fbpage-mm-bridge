FROM rust:1.88-slim-bookworm as builder

WORKDIR /app

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock* ./
COPY ../customer-service/Cargo.toml ./services/customer-service/
COPY ../message-service/Cargo.toml ./services/message-service/
COPY Cargo.toml ./services/facebook-graph-service/

RUN mkdir -p src && echo "fn main() {}" > src/main.rs
RUN mkdir -p services/customer-service/src && echo 'fn main() {}' > services/customer-service/src/main.rs
RUN mkdir -p services/message-service/src && echo 'fn main() {}' > services/message-service/src/main.rs
RUN mkdir -p services/facebook-graph-service/src && echo 'fn main() {}' > services/facebook-graph-service/src/main.rs

RUN cargo build --release --package facebook-graph-service && rm -rf src services/*/src

COPY migrations migrations
COPY src services/facebook-graph-service/src

RUN mkdir -p src services/customer-service/src services/message-service/src services/facebook-graph-service/src && \
    touch src/main.rs services/customer-service/src/main.rs services/message-service/src/main.rs services/facebook-graph-service/src/main.rs && \
    cargo build --release --package facebook-graph-service

FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/facebook-graph-service /usr/local/bin/

COPY migrations ./migrations

COPY .env.example .env

EXPOSE 3003

CMD ["facebook-graph-service"]
