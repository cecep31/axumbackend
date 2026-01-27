# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build, Test, and Run Commands

```bash
cargo build                    # Debug build
cargo build --release          # Release build
cargo run                      # Run development server

cargo test                     # Run all tests
cargo test test_name           # Run specific test by name
cargo test -- --nocapture      # Run tests with output

cargo clippy                   # Run linter
cargo clippy --fix             # Auto-apply linter fixes
cargo check                    # Type-check without building
cargo fmt                      # Format code
cargo fmt --check              # Check formatting
```

## Architecture Overview

This is a Rust web backend using Axum v0.8.8 with PostgreSQL, implementing a REST API for blog post management.

### Layer Structure

```
handlers/  →  services/  →  database (tokio-postgres + deadpool)
                ↓
            models/ (Post, User, Tag)
```

- **handlers/**: HTTP layer - extractors, query params, route definitions
- **services/**: Business logic - raw SQL queries via `tokio_postgres::Client`
- **models/**: Data structures with `From<Row>` implementations
- **database.rs**: Connection pooling with `deadpool_postgres`

### Request Flow

1. Handler extracts state (`State<DbPool>`) and parameters
2. Gets client: `pool.get().await?`
3. Passes client to service functions
4. Services execute parameterized queries, return `Result<Vec<T>, tokio_postgres::Error>`
5. Handler wraps response in `ApiResponse<T>` with `Json()`

### Key Types

- **DbPool**: `deadpool_postgres::Pool` type alias
- **ApiResponse<T>**: Wrapper with `success`, `data`, and `meta` fields
- **AppError**: Enum with Database, Pool, NotFound, BadRequest, InternalServerError variants

### Error Handling

- `AppError` implements `IntoResponse` returning JSON: `{"success": false, "error": "...", "data": null}`
- `From<tokio_postgres::Error>` and `From<PoolError>` impls for `?` operator
- Log with `tracing::error!` for database errors

### Routes

All routes prefixed with `/v1`:
- `GET /v1/posts` - paginated posts with search/sort
- `GET /v1/posts/random` - random posts
- `GET /v1/posts/tag/{tag}` - posts by tag
- `GET /v1/posts/u/{username}/{slug}` - single post by user/slug

### Configuration

Environment variables (see `.env.example`):
- `DATABASE_URL` - PostgreSQL connection string
- `PORT` - server port (default 8000)
- `DB_POOL_*` - connection pool settings
