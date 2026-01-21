# Build stage
FROM rust:1-alpine AS builder

WORKDIR /build

# Install build dependencies
# musl-dev: required for Rust compilation on Alpine
# postgresql-dev: required for tokio-postgres crate
RUN apk add --no-cache musl-dev postgresql-dev

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
FROM alpine:3.19 AS production

# Install runtime dependencies
# libpq: PostgreSQL client library (required for tokio-postgres)
# binutils: required for strip command
# ca-certificates: required if app makes HTTPS requests
RUN apk add --no-cache libpq binutils ca-certificates

# Create non-root user
RUN addgroup -g 1000 app && adduser -u 1000 -G app -s /bin/sh -D app

# Copy binary from builder and strip it
COPY --from=builder /build/target/release/axumbackend /usr/local/bin/
RUN strip /usr/local/bin/axumbackend

# Switch to non-root user
USER app

# Expose port
EXPOSE 8000

# Run the application
CMD ["axumbackend"]
