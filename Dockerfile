# Build stage
FROM rust:1-alpine AS builder

WORKDIR /build

# Install build dependencies
RUN apk add --no-cache musl-dev postgresql-dev openssl-dev

# Copy dependency files first for better caching
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && touch src/main.rs && cargo build --release

# Copy source code and build
COPY src ./src
RUN cargo build --release

# Production stage
FROM alpine:3.19 AS production

# Install runtime dependencies
RUN apk add --no-cache libpq openssl ca-certificates

# Create non-root user
RUN addgroup -g 1000 app && adduser -u 1000 -G app -s /bin/sh -D app

# Copy binary from builder
COPY --from=builder /build/target/release/rocketbackend /usr/local/bin/

# Copy .env file if it exists
COPY --chown=app:app .env ./

# Switch to non-root user
USER app

# Expose port (default Rocket port)
EXPOSE 8000

# Set environment variables
ENV ROCKET_PORT=8000
ENV ROCKET_ADDRESS=0.0.0.0

# Run the application
CMD ["rocketbackend"]
