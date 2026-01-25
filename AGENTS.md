# AGENTS.md - Axum Backend Project

## Project Overview

This is a Rust web backend application built with the Axum web framework (v0.8.8) using PostgreSQL. The project implements a blog post management system with REST API endpoints. Uses Rust edition 2024.

## Build, Lint, and Test Commands

```bash
# Build the project
cargo build

# Build in release mode
cargo build --release

# Run the application
cargo run

# Run all tests
cargo test

# Run a single test by name (exact match)
cargo test test_name

# Run tests matching a pattern
cargo test test_pattern

# Run tests with output
cargo test -- --nocapture

# Run clippy linter
cargo clippy

# Run clippy with fixes (auto-apply)
cargo clippy --fix

# Check code without building
cargo check

# Format code
cargo fmt

# Check formatting
cargo fmt --check
```

## Code Style Guidelines

### General Principles
- Write clean, idiomatic Rust code following the 2024 edition conventions
- Prefer explicit error handling over unwrap/panic in production code
- Use async/await for all I/O operations (database, HTTP)
- Use `Arc<Client>` for shared database connection state

### Imports and Module Structure
- Organize code into modules: `models/`, `handlers/`, `services/`, `database.rs`
- Each module has a `mod.rs` that exports submodules
- Use absolute imports with `crate::` for internal modules
- Group imports by crate (std, external, internal)
- Use `use` statements at top level, not `#[macro_use]`

### Naming Conventions
- **Files**: snake_case (e.g., `post_handler.rs`, `health_check.rs`)
- **Structs**: PascalCase (e.g., `Post`, `User`, `ApiResponse`)
- **Functions**: snake_case (e.g., `get_all_posts`, `connect`)
- **Variables**: snake_case (e.g., `db_conn`, `post_id`)
- **Constants**: SCREAMING_SNAKE_CASE for global constants
- **Modules**: snake_case
- **Types in function signatures**: Use explicit types, avoid inference

### Formatting and Style
- Use default Rustfmt settings (no custom config)
- Maximum line length: 100 characters (default)
- Use 4 spaces for indentation
- Place opening braces on same line as declaration
- Add trailing comma in multi-line expressions

### Error Handling
- Return `Result<T, tokio_postgres::Error>` for database operations
- Use `?` operator for propagating errors in async contexts
- Use `unwrap_or_else` or `unwrap_or` for fallible operations with defaults
- Log errors with `tracing::error!` for connection/background errors
- Handle errors gracefully in handlers with fallbacks to empty collections
- Define `AppError` enum with variants: Database, Pool, NotFound, BadRequest, InternalServerError
- Implement `IntoResponse` for custom errors returning JSON with `success: false`
- Use `From<tokio_postgres::Error>` and `From<PoolError>` for automatic error conversion
- Handle connection pool errors separately with `deadpool_postgres::PoolError`

### Logging and Tracing
- Initialize tracing with `tracing_subscriber::registry()` and EnvFilter
- Default log level is info, with tower_http at info and axum::rejection at trace
- Use `tracing::info!`, `tracing::error!` macros throughout the application
- Log server startup with address and any important events

### Async and Concurrency
- All database operations are async using tokio-postgres with deadpool connection pooling
- Use `tokio::spawn` for background connection handling
- Use `DbPool` (deadpool::Pool) for connection pool management in Axum state
- Use `State<DbPool>` for DI in route handlers
- Get client from pool with `pool.get().await` in handlers

### Types and Serialization
- Use `serde::{Serialize, Deserialize}` for all serializable types
- Use `Json<T>` from axum for JSON response types in routes
- Use `uuid::Uuid` for unique identifiers
- Use `chrono::DateTime<Utc>` for timestamps
- Clone derives are acceptable for simple data types

### Axum Framework Patterns
- Mount routes under `/v1` prefix: `Router::new().route("/v1/posts", get(handler))`
- Use `routing::get/post` extractors for defining routes
- State management: `Router::with_state(Arc::new(client))`
- Health check endpoint at `GET /health` returning `"OK"`
- Use `Query<T>` for query parameters, `Json<T>` for request bodies
- Add CORS with `tower_http::cors::CorsLayer::permissive()`
- Add `TraceLayer` for request logging: `.layer(TraceLayer::new_for_http())`
- Merge route groups with `.merge(sub_router)` in handlers/mod.rs

### API Response Patterns
- Use `ApiResponse<T>` wrapper struct with `success: bool`, `data: T`, `error: Option<String>`
- All successful responses return `Json(ApiResponse::success(data))`
- Error responses handled via `AppError` with `IntoResponse` returning `success: false` JSON

### Database Queries
- Use parameterized queries with `$1`, `$2` placeholders
- Use `JOIN` statements for related data (posts with users)
- Return `Result<Vec<T>, tokio_postgres::Error>` from service functions
- Handle connection errors in background spawn with `tokio::spawn`
- Handle truncation logic in services (e.g., body to 200 chars)

### Security
- Never commit secrets; use `.env` files with `dotenvy`
- DATABASE_URL is loaded from environment with fallback defaults
- Validate all query parameters (e.g., `limit: Option<i64>`)
- Use parameterized queries to prevent SQL injection

### Testing
- Place tests in same file using `#[cfg(test)]` module
- Use `#[test]` attribute for test functions
- Run single tests with `cargo test function_name`
- Mock database connections for unit tests

### Git Workflow
- Commit messages should be concise, imperative mood
- No force pushes to main without explicit approval
- Run `cargo clippy` and `cargo fmt` before committing
- Never commit generated files (database.db, target/)

## Key File Locations

- **Entry point**: `src/main.rs`
- **Database**: `src/database.rs`
- **Error handling**: `src/error.rs`
- **API response**: `src/response.rs`
- **Config**: `src/config.rs`, `.env`
- **Models**: `src/models/{post,user,tag}.rs`
- **Handlers**: `src/handlers/{health,post,tag}.rs`
- **Services**: `src/services/{post,tag}.rs`
