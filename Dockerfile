# Build stage
FROM rust:1-trixie AS builder

WORKDIR /build

# Install build dependencies
# libpq-dev: required for tokio-postgres crate
# pkg-config: required for linking C libraries
RUN apt-get update && apt-get install -y libpq-dev pkg-config && rm -rf /var/lib/apt/lists/*

# Create a dummy main.rs to cache dependencies
RUN mkdir src
RUN echo "fn main() {}" > src/main.rs

# Copy dependency files first for better caching
COPY Cargo.toml Cargo.lock ./
RUN cargo build --release

# Remove dummy source and copy actual source code
RUN rm -rf src
COPY src ./src

# Touch main.rs to invalidate cargo cache and rebuild with actual source
RUN touch src/main.rs
RUN cargo build --release

# Production stage
FROM debian:trixie-slim AS production

# Install runtime dependencies
# libpq5: PostgreSQL client library (required for tokio-postgres)
# binutils: required for strip command
# ca-certificates: required if app makes HTTPS requests
RUN apt-get update && apt-get install -y libpq5 binutils ca-certificates && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd -g 1000 app && useradd -u 1000 -g app -s /bin/sh -m -d /home/app app

# Copy binary from builder and strip it
COPY --from=builder /build/target/release/axumbackend /usr/local/bin/
RUN strip /usr/local/bin/axumbackend

# Switch to non-root user
USER app

# Expose port
EXPOSE 8080

# Run the application
CMD ["axumbackend"]
