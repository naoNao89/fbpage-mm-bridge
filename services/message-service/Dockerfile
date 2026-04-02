FROM rust:1.88-bookworm as builder

WORKDIR /app

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock* ./
COPY ../customer-service/Cargo.toml ./services/customer-service/
COPY ../message-service/Cargo.toml ./services/message-service/
COPY ../facebook-graph-service/Cargo.toml ./services/facebook-graph-service/

RUN mkdir -p src && echo 'fn main() {}' > src/main.rs
RUN mkdir -p services/customer-service/src && echo 'fn main() {}' > services/customer-service/src/main.rs
RUN mkdir -p services/message-service/src && echo 'fn main() {}' > services/message-service/src/main.rs
RUN mkdir -p services/facebook-graph-service/src && echo 'fn main() {}' > services/facebook-graph-service/src/main.rs

RUN cargo build --release --package message-service && rm -rf src services/*/src

COPY migrations migrations
COPY src services/message-service/src

RUN mkdir -p src services/customer-service/src services/message-service/src services/facebook-graph-service/src && \
    touch src/main.rs services/customer-service/src/main.rs services/message-service/src/main.rs services/facebook-graph-service/src/main.rs && \
    cargo build --release --package message-service

FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y ca-certificates libssl3 curl && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/message-service /app/message-service

COPY migrations ./migrations

RUN useradd -r -s /bin/false appuser && \
    chown -R appuser:appuser /app

USER appuser

EXPOSE 3002

ENV BIND_ADDRESS=0.0.0.0:3002
ENV LOG_LEVEL=info

CMD ["./message-service"]
