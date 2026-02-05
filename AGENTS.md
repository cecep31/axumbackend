# AGENTS.md - Axum Backend Project

## Project Overview

Rust web backend with Axum v0.8.8, PostgreSQL, and REST API for blog post management. Uses Rust edition 2024.

## Build, Lint, and Test Commands

```bash
# Build and run
cargo build
cargo build --release
cargo run

# Testing
cargo test                          # all tests
cargo test test_name                # single test (exact match)
cargo test test_pattern             # pattern match
cargo test -- --nocapture           # with output

# Linting and formatting
cargo clippy                        # linter
cargo clippy --fix                  # auto-apply fixes
cargo check                         # check without building
cargo fmt                           # format
cargo fmt --check                   # check formatting
```

## Code Style Guidelines

### General
- Write idiomatic Rust 2024 edition code
- Prefer explicit error handling over unwrap/panic
- Use async/await for all I/O operations

### Imports and Modules
- Organize: `models/`, `handlers/`, `services/`, `database.rs`
- Each module has `mod.rs` exporting submodules
- Use `crate::` for absolute imports
- Group imports: std, external, internal
- Example:
  ```rust
  use std::collections::HashMap;
  use tokio_postgres::Client;
  use crate::models::post::Post;
  ```

### Naming Conventions
- **Files**: snake_case (`post_handler.rs`)
- **Structs**: PascalCase (`Post`, `ApiResponse`)
- **Functions**: snake_case (`get_all_posts`)
- **Variables**: snake_case (`db_conn`, `post_id`)
- **Constants**: SCREAMING_SNAKE_CASE with `const`
- **Types**: Explicit in function signatures

### Formatting
- Default Rustfmt settings (4 spaces, 100 char width)
- Opening braces on same line
- Trailing commas in multi-line expressions

### Error Handling
- Return `Result<T, tokio_postgres::Error>` for DB operations
- Use `?` for error propagation
- `AppError` enum: Database, Pool, NotFound, BadRequest, InternalServerError
- Implement `IntoResponse` returning JSON: `{"success": false, "error": "...", "data": null}`
- Handle `deadpool_postgres::PoolError` separately
- Log with `tracing::error!` for background errors

### Logging and Tracing
- Initialize with `tracing_subscriber::registry()` and EnvFilter
- Default level: info (tower_http: info, axum::rejection: trace)
- Use `tracing::info!`, `tracing::error!` macros

### Async and Concurrency
- Use `tokio-postgres` with deadpool connection pooling
- `DbPool` (deadpool::Pool) in Axum state
- `State<DbPool>` for DI in handlers
- Get client: `pool.get().await?`

### Types and Serialization
- `serde::{Serialize, Deserialize}` for all serializable types
- `Json<T>` from axum for responses
- `uuid::Uuid` for IDs, `chrono::DateTime<Utc>` for timestamps
- Models implement `From<&Row>` for database deserialization
- Clone derives acceptable for simple types

### Axum Framework Patterns
- Routes under `/v1` prefix
- `Query<T>` for query params, `Json<T>` for request bodies
- CORS: `CorsLayer::permissive()`
- Trace logging: `TraceLayer::new_for_http()`
- Merge routers with `.merge(sub_router)`
- Handler returns: `Result<Json<ApiResponse<T>>, AppError>`

### API Response Patterns
- `ApiResponse<T>` wrapper: `success: bool`, `data: Option<T>`, `meta: Meta`
- Meta contains: `total_items`, `offset`, `limit`, `total_pages`
- Success: `Json(ApiResponse::success(data))`
- With pagination: `Json(ApiResponse::with_meta(data, total, limit, offset))`
- Error: `AppError` with `IntoResponse`

### Database
- Parameterized queries: `$1`, `$2`
- Use `JOIN` for related data (e.g., `posts` JOIN `users`)
- Escape LIKE patterns: replace `\`, `%`, `_` to prevent injection
- Validate `order_by` fields against whitelist using match
- Return `Result<(Vec<T>, i64), tokio_postgres::Error>` (data + total count)
- Batch fetch tags to avoid N+1 queries using `ANY($1)` array parameter

### Validation
- Use `axum-valid` with `validator` for request validation
- Wrap extractors with `Valid`: `Valid(Query<T>)`, `Valid(Path<T>)`
- Define validation rules using derive macro:
  ```rust
  #[derive(Deserialize, Validate)]
  pub struct PaginationQuery {
      #[validate(range(min = 0, max = 10_000))]
      offset: Option<i64>,
      #[validate(range(min = 1, max = 100))]
      limit: Option<i64>,
  }
  ```
- Use regex validation for path parameters with `once_cell::Lazy`

### Security
- Use `.env` files with `dotenvy` for secrets
- Validate all query parameters using `axum-valid`
- Escape LIKE pattern characters to prevent injection
- Parameterized queries prevent SQL injection

### Testing
- Tests in `#[cfg(test)]` module in same file
- Mock database connections for unit tests

### Git Workflow
- Imperative commit messages
- No force pushes to main
- Run `cargo clippy` and `cargo fmt` before committing
- Never commit: `.env`, `database.db`, `target/`

## Key Files

| Component | Location |
|-----------|----------|
| Entry point | `src/main.rs` |
| Database | `src/database.rs` |
| Error handling | `src/error.rs` |
| API response | `src/response.rs` |
| Config | `src/config.rs`, `.env` |
| Models | `src/models/{post,user,tag}.rs` |
| Handlers | `src/handlers/{health,post,tag}.rs` |
| Services | `src/services/{post,tag}.rs` |

## Environment Variables

```bash
DATABASE_URL=host=localhost user=postgres password=postgres dbname=axumbackend
PORT=8000
DB_POOL_MAX_SIZE=20
DB_POOL_CONNECTION_TIMEOUT=30
```
