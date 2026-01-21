# Build stage
FROM rust:1-alpine AS builder

WORKDIR /build

# Install build dependencies
RUN apk add --no-cache musl-dev postgresql-dev openssl-dev

# Create a dummy main.rs to cache dependencies
RUN mkdir src
RUN echo "fn main() {}" > src/main.rs

# Copy dependency files first for better caching
COPY Cargo.toml Cargo.lock ./
RUN cargo build --release

# Copy source code and build
COPY src ./src
RUN cargo build --release

# Production stage
FROM alpine:3.19 AS production

# Install runtime dependencies
RUN apk add --no-cache libpq openssl ca-certificates

# Create non-root user
RUN addgroup -g 1000 app && adduser -u 1000 -G app -s /bin/sh -D app

# Copy binary from builder and strip it
COPY --from=builder /build/target/release/rocketbackend /usr/local/bin/
RUN strip /usr/local/bin/rocketbackend

# Switch to non-root user
USER app

# Expose port
EXPOSE 8000

# Run the application
CMD ["rocketbackend"]
