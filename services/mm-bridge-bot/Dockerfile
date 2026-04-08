FROM rust:1.88-bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace root files
COPY Cargo.toml Cargo.lock ./

# Copy all workspace member Cargo.toml files
COPY services/customer-service/Cargo.toml services/customer-service/
COPY services/message-service/Cargo.toml services/message-service/
COPY services/facebook-graph-service/Cargo.toml services/facebook-graph-service/
COPY services/mm-bridge-bot/Cargo.toml services/mm-bridge-bot/

# Create dummy source files so cargo can resolve the workspace
RUN mkdir -p services/customer-service/src && \
    mkdir -p services/message-service/src && \
    mkdir -p services/facebook-graph-service/src && \
    mkdir -p services/mm-bridge-bot/src && \
    echo 'fn main() {}' > services/customer-service/src/main.rs && \
    echo 'fn main() {}' > services/message-service/src/main.rs && \
    echo 'fn main() {}' > services/facebook-graph-service/src/main.rs && \
    echo 'fn main() {}' > services/mm-bridge-bot/src/main.rs

# Build dependencies only (this layer gets cached)
RUN cargo build --release --package mm-bridge-bot

# Now overwrite with real source code
COPY services/mm-bridge-bot/src services/mm-bridge-bot/src

# Touch the file to break cargo cache so it recompiles with real code
RUN touch services/mm-bridge-bot/src/main.rs

# Rebuild with real source
RUN cargo build --release --package mm-bridge-bot

FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/mm-bridge-bot /app/mm-bridge-bot

CMD ["/app/mm-bridge-bot"]