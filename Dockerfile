# Multi-stage Docker build for Apollo MCP Server with Auth0 Phase 2 support
#
# 260934 #57: base must be >= 1.85 — every crate declares `edition = "2024"`
# (see crates/*/Cargo.toml), which Rust 1.83 cannot compile ("feature
# `edition2024` is required"). The previous `rust:1.83-bookworm` pin was the
# reason monovandi/itopsai-mcp couldn't publish. The `rust:1-bookworm` tag
# tracks the latest stable 1.x, matching this repo's rust-toolchain.toml
# (`channel = "stable"`) so the base image and the in-repo toolchain stay in
# lockstep and a future edition bump can't re-break the image build.
FROM rust:1-bookworm AS builder

# Install dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /app

# Copy workspace files
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ ./crates/

# Build the application
RUN cargo build --release --bin apollo-mcp-server

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies and debugging tools
# TODO: Remove debugging tools for production release
RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    wget \
    netcat-traditional \
    net-tools \
    procps \
    lsof \
    telnet \
    && rm -rf /var/lib/apt/lists/*

# Create user for security
RUN groupadd -r apollo && useradd -r -g apollo apollo

# Create directories for configuration and data
RUN mkdir -p /config /data && chown apollo:apollo /config /data

# Copy binary from builder stage
COPY --from=builder /app/target/release/apollo-mcp-server /usr/local/bin/apollo-mcp-server

# Set working directory
WORKDIR /app

# Switch to non-root user
USER apollo

# Environment variables for clean MCP protocol communication
ENV NO_COLOR=1
ENV RUST_LOG=error

# Default environment variables for Streamable HTTP transport
ENV APOLLO_MCP_TRANSPORT__TYPE=streamable_http
ENV APOLLO_MCP_TRANSPORT__ADDRESS=0.0.0.0
ENV APOLLO_MCP_TRANSPORT__PORT=5000

# Expose port 5000
EXPOSE 5000

# Set entrypoint - expects config file as argument
ENTRYPOINT ["apollo-mcp-server"]
CMD ["/config/server-config.yaml"]