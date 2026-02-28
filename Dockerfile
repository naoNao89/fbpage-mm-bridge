# Build stage
FROM rust:1.83-alpine AS builder

WORKDIR /app

# Install build dependencies
RUN apk add --no-cache \
    musl-dev \
    openssl-dev \
    pkgconfig

# Copy manifests
COPY Cargo.toml Cargo.lock* ./

# Create dummy src for caching
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs

# Build dependencies (cached layer)
RUN cargo build --release && rm -rf src

# Copy source code
COPY src ./src

# Build application
RUN cargo build --release

# Runtime stage
FROM alpine:3.19

WORKDIR /app

# Install runtime dependencies
RUN apk add --no-cache \
    ca-certificates \
    libssl3

# Copy binary from builder
COPY --from=builder /app/target/release/fbpage-mm-bridge /app/fbpage-mm-bridge

# Create non-root user
RUN addgroup -S appgroup && adduser -S appuser -G appgroup
USER appuser

EXPOSE 3000

CMD ["/app/fbpage-mm-bridge"]
