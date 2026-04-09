# Gemini Context: Axum Backend

This project is a high-performance Rust web backend built with the Axum framework, designed for managing blog posts with a PostgreSQL database.

## Project Overview

- **Core Framework**: [Axum v0.8.8](https://docs.rs/axum/0.8.8/axum/)
- **Runtime**: [Tokio](https://tokio.rs/)
- **Database**: PostgreSQL with [deadpool-postgres](https://docs.rs/deadpool-postgres/) for connection pooling and [tokio-postgres](https://docs.rs/tokio-postgres/) for async queries.
- **Validation**: [axum-valid](https://docs.rs/axum-valid/) with [validator](https://docs.rs/validator/) for request data integrity.
- **Architecture**: Clean, layered separation of concerns (Handlers -> Services -> Models -> Database).

## Building and Running

### Prerequisites
- Rust toolchain (Edition 2024)
- PostgreSQL database
- Environment variables configured (see `.env.example`)

### Key Commands
```bash
# Development
cargo run              # Start the development server
cargo check            # Fast type-checking
cargo watch -x run     # (Optional) Auto-reload on changes (requires cargo-watch)

# Testing & Quality
cargo test             # Run the test suite
cargo clippy           # Run the linter
cargo fmt              # Format the codebase

# Production
cargo build --release  # Build optimized binary
docker build -t axumbackend . # Build Docker image
```

## Architecture & Directory Structure

- `src/main.rs`: Application entry point; initializes tracing, config, database pool, and the Axum server.
- `src/handlers/`: HTTP layer. Defines routes, extracts request data, validates inputs, and calls services.
- `src/services/`: Business logic and persistence layer. Contains raw SQL queries and data manipulation logic.
- `src/models/`: Data structures. Defines the domain entities (Post, User, Tag) and their mapping from database rows.
- `src/config.rs`: Centralized configuration management using environment variables.
- `src/database.rs`: Database connection pool setup and management.
- `src/error.rs`: Centralized error handling using a custom `AppError` enum that implements `IntoResponse`.
- `src/response.rs`: Standardized generic `ApiResponse<T>` wrapper for consistent API output.

## Development Conventions

### Coding Standards
- **Surgical Updates**: When modifying existing logic, maintain the established architectural patterns.
- **Error Handling**: Prefer the `?` operator for error propagation. All errors should eventually map to `AppError`.
- **Naming**: Follow standard Rust naming conventions (PascalCase for types, snake_case for functions/variables).
- **SQL Patterns**: 
    - Always use parameterized queries to prevent SQL injection.
    - Avoid N+1 query problems by using batch fetching (e.g., `fetch_tags_for_posts`).
    - Use `ILIKE` for case-insensitive searches where appropriate.
- **Validation**: Every public API endpoint that accepts input MUST use `Valid<Query<...>>` or `Valid<Json<...>>`.

### API Response Format
All successful responses return:
```json
{
  "success": true,
  "data": { ... },
  "meta": { "total_items": 10, "offset": 0, "limit": 10 }
}
```

### Logging
- Use the `tracing` crate (`info!`, `warn!`, `error!`).
- The `TraceLayer` middleware is active by default to log all HTTP requests.

## Tech Stack Summary

| Library | Version | Purpose |
|---------|---------|---------|
| `axum` | 0.8.8 | Web Framework |
| `tokio` | 1.x | Async Runtime |
| `deadpool-postgres` | 0.14 | DB Pooling |
| `serde` | 1.0 | JSON Serialization |
| `validator` | 0.20 | Data Validation |
| `tower-http` | 0.5 | CORS, Tracing Middleware |
| `chrono` | 0.4 | Time & Date Handling |
| `uuid` | 1.x | UUID support |
