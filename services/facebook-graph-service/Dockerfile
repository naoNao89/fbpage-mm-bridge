FROM rust:1.88-slim-bookworm as builder

WORKDIR /app

# Install dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests
COPY Cargo.toml Cargo.lock* ./

# Create dummy source for dependency caching
RUN mkdir -p src && echo "fn main() {}" > src/main.rs

# Build dependencies (with dummy main)
RUN cargo build --release && rm -rf src

# Copy migrations for sqlx compile-time check
COPY migrations ./migrations

# Copy actual source
COPY src ./src

# Build the application
RUN touch src/main.rs && cargo build --release

# Runtime image
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy binary from builder
COPY --from=builder /app/target/release/facebook-graph-service /usr/local/bin/

# Copy migrations
COPY --from=builder /app/migrations ./migrations

# Copy .env.example as default
COPY .env.example .env

EXPOSE 3003

CMD ["facebook-graph-service"]
