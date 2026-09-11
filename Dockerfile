# syntax=docker/dockerfile:1

# ==================== Chef Base ====================
# Pin Rust version for reproducible builds
FROM rust:1.98-trixie AS chef
WORKDIR /build

# Install cargo-chef for proper dependency caching
# This layer is cached as long as the base image doesn't change
RUN cargo install cargo-chef --locked

# ==================== Planner Stage ====================
# Generate recipe.json containing the dependency manifest
FROM chef AS planner

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo chef prepare --recipe-path recipe.json

# ==================== Builder Stage ====================
FROM chef AS builder

# This layer is cached as long as recipe.json doesn't change
# (dependencies are NOT rebuilt when only source code changes)
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json

# Build the application with the actual source code
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Build and strip the binary in one layer to reduce size
RUN cargo build --release --locked \
    && strip target/release/axumbackend

# ==================== Production Stage ====================
FROM debian:trixie-slim AS production

# Install only the required runtime dependencies:
# - ca-certificates: for HTTPS requests (reqwest)
# - curl: for the healthcheck
# libpq5 is not needed since SeaORM uses rustls (pure Rust)
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/* \
    && apt-get clean

# Create a non-root user for security
RUN groupadd -g 1000 app && useradd -u 1000 -g app -s /bin/sh -m -d /home/app app

# Copy the stripped binary from the builder
COPY --from=builder /build/target/release/axumbackend /usr/local/bin/axumbackend

# Switch to the non-root user
USER app

# Expose the port (default 8080, can be overridden via the PORT env var)
EXPOSE 8080

# Healthcheck using the existing /health endpoint
# Shell form is used to support ${PORT:-8080} variable expansion
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD curl -fsS "http://localhost:${PORT:-8080}/health" || exit 1

# Run the application
CMD ["axumbackend"]
