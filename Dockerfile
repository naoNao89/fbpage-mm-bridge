FROM rust:1.83-alpine AS builder

WORKDIR /app

RUN apk add --no-cache \
    musl-dev \
    openssl-dev \
    pkgconfig

COPY Cargo.toml Cargo.lock* ./
COPY services/customer-service/Cargo.toml services/customer-service/
COPY services/message-service/Cargo.toml services/message-service/
COPY services/facebook-graph-service/Cargo.toml services/facebook-graph-service/

RUN mkdir -p src && echo 'fn main() {}' > src/main.rs
RUN mkdir -p services/customer-service/src && echo 'fn main() {}' > services/customer-service/src/main.rs
RUN mkdir -p services/message-service/src && echo 'fn main() {}' > services/message-service/src/main.rs
RUN mkdir -p services/facebook-graph-service/src && echo 'fn main() {}' > services/facebook-graph-service/src/main.rs

RUN cargo build --release && rm -rf src services/*/src

COPY src ./src

RUN cargo build --release

FROM alpine:3.19

WORKDIR /app

RUN apk add --no-cache \
    ca-certificates \
    libssl3

COPY --from=builder /app/target/release/fbpage-mm-bridge /app/fbpage-mm-bridge

RUN addgroup -S appgroup && adduser -S appuser -G appgroup
USER appuser

EXPOSE 3000

CMD ["/app/fbpage-mm-bridge"]
