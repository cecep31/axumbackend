# Axum Backend Project

## Project Overview

This is a Rust-based web backend application built with the Axum web framework (v0.8.8). The project implements a blog post management system with PostgreSQL database integration. Key features include:

- REST API endpoints for health checking, blog posts, and tags
- PostgreSQL database with connection pooling (deadpool)
- Structured architecture with separation of concerns (models, handlers, services, database layer)
- Built with Rust 2024 edition
- Input validation using `axum-valid` and `validator` crates
- CORS and request tracing middleware

### Architecture

The project follows a modular architecture with the following components:

- **Main Application (`src/main.rs`)**: Initializes the Axum web server, establishes database connection pool, and configures middleware
- **Configuration (`src/config.rs`)**: Handles application configuration from environment variables with sensible defaults
- **Database Layer (`src/database.rs`)**: Creates and manages PostgreSQL connection pool using deadpool-postgres
- **Error Handling (`src/error.rs`)**: Defines custom `AppError` enum with `IntoResponse` implementation for consistent error responses
- **Response Format (`src/response.rs`)**: Generic `ApiResponse<T>` wrapper with pagination metadata
- **Models (`src/models/`)**: Data structures (Post, User, Tag) with serialization and database row mapping
- **Handlers (`src/handlers/`)**: HTTP endpoint handlers with input validation and routing
- **Services (`src/services/`)**: Business logic layer for data operations

### Dependencies

| Dependency | Version | Purpose |
|------------|---------|---------|
| `axum` | 0.8.8 | Web framework for routing and HTTP handling |
| `axum-valid` | 0.24 | Request validation integration |
| `tokio-postgres` | 0.7.17 | PostgreSQL async client |
| `deadpool-postgres` | 0.14 | Connection pooling |
| `serde` | 1.0 | Serialization/deserialization |
| `chrono` | 0.4.43 | Date/time handling with UTC |
| `uuid` | 1.20.0 | UUID generation and handling |
| `tower-http` | 0.5.2 | HTTP middleware (CORS, tracing) |
| `validator` | 0.20 | Validation derive macros |
| `tracing` | 0.1 | Logging and tracing |

## Building and Running

### Prerequisites

- Rust toolchain (edition 2024)
- Cargo package manager
- PostgreSQL database server

### Build Commands

```bash
# Build the project
cargo build

# Build in release mode
cargo build --release

# Run the application
cargo run

# Check without building
cargo check

# Format code
cargo fmt

# Check formatting
cargo fmt --check

# Run linter
cargo clippy

# Run tests
cargo test
```

### Environment Variables

Create a `.env` file in the project root with the following variables:

```bash
# Server Configuration
PORT=8080

# Database Configuration
DATABASE_URL="postgresql://postgres:password@localhost:5432/axumbackend"

# Connection Pool Configuration
DB_POOL_MAX_SIZE=20
DB_POOL_CONNECTION_TIMEOUT=30
DB_POOL_MAX_LIFETIME=1800
DB_POOL_IDLE_TIMEOUT=600
```

### Running the Application

The application connects to a PostgreSQL database specified by the `DATABASE_URL` environment variable. The server listens on the configured port (defaults to 8080).

## API Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/` | Health check |
| `GET` | `/health` | Health check |
| `GET` | `/v1/posts` | Get all published posts (paginated) |
| `GET` | `/v1/posts/random?limit=N` | Get N random posts (default: 6) |
| `GET` | `/v1/posts/tag/{tag}` | Get posts by tag name |
| `GET` | `/v1/posts/u/{username}/{slug}` | Get specific post by author and slug |

### Query Parameters

**Posts endpoints** (`/v1/posts`, `/v1/posts/tag/{tag}`):
- `offset` (optional, default: 0): Pagination offset (0-10000)
- `limit` (optional, default: 10): Items per page (1-100)
- `search` (optional): Search term for title, body, or username
- `order_by` (optional): Sort field (id, title, created_at, updated_at, view_count, like_count, bookmark_count)
- `order_direction` (optional): Sort direction (asc, desc)

**Random posts endpoint** (`/v1/posts/random`):
- `limit` (optional, default: 6): Number of random posts to fetch (1-100)

### Response Format

All successful responses follow this structure:

```json
{
  "success": true,
  "data": { ... },
  "meta": {
    "total_items": 100,
    "offset": 0,
    "limit": 10,
    "total_pages": 10
  }
}
```

Error responses:

```json
{
  "success": false,
  "error": "Error message here",
  "data": null
}
```

## Development Conventions

### Code Structure

- Modules organized by concern: `models/`, `handlers/`, `services/`
- Each module has `mod.rs` for exports
- Handlers delegate business logic to service functions
- Database connections managed through Axum's state system with connection pool

### Naming Conventions

- **Files**: snake_case (`post.rs`, `health.rs`)
- **Structs**: PascalCase (`Post`, `ApiResponse`, `AppError`)
- **Functions**: snake_case (`get_all_posts`, `create_pool`)
- **Variables**: snake_case (`db_pool`, `post_id`)
- **Constants**: SCREAMING_SNAKE_CASE with `const`

### Error Handling

- `AppError` enum variants: `Database`, `Pool`, `NotFound`, `BadRequest`, `InternalServerError`
- Use `?` operator for error propagation
- Implement `From` traits for automatic conversion
- Log database errors with `tracing::error!`
- Return `Result<Json<ApiResponse<T>>, AppError>` from handlers

### Input Validation

- Use `axum-valid` with `validator` derive macros
- Wrap extractors: `Valid(Query<T>)`, `Valid(Path<T>)`
- Regex validation for path parameters using `once_cell::sync::Lazy`
- Validation rules via `#[validate(...)]` attributes

### Database Patterns

- Parameterized queries to prevent SQL injection
- Escape LIKE pattern characters (`%`, `_`, `\`)
- Validate `order_by` fields against whitelist
- Batch fetch tags to avoid N+1 queries
- Return `(Vec<T>, i64)` for data + total count

### Logging

- Initialize with `tracing_subscriber::registry()` and `EnvFilter`
- Default levels: info for app, info for tower_http, trace for axum rejection
- Use `tracing::info!`, `tracing::error!` macros

## File Structure

```
axumbackend/
├── Cargo.toml              # Project manifest and dependencies
├── Cargo.lock              # Dependency lock file
├── Dockerfile              # Multi-stage Docker build
├── .dockerignore           # Docker build exclusions
├── .gitignore              # Git exclusions
├── .env.example            # Example environment variables
├── AGENTS.md               # Agent instructions
├── QWEN.md                 # This file
├── src/
│   ├── main.rs             # Application entry point
│   ├── config.rs           # Environment configuration
│   ├── database.rs         # Database pool setup
│   ├── error.rs            # Error types and responses
│   ├── response.rs         # API response wrapper
│   ├── handlers/
│   │   ├── mod.rs          # Router creation and middleware
│   │   ├── health.rs       # Health check endpoints
│   │   ├── post.rs         # Post endpoints and validation
│   │   └── tag.rs          # Tag endpoints
│   ├── models/
│   │   ├── mod.rs          # Model exports
│   │   ├── post.rs         # Post struct with Row mapping
│   │   ├── user.rs         # User struct
│   │   └── tag.rs          # Tag struct
│   └── services/
│       ├── mod.rs          # Service exports
│       ├── post.rs         # Post business logic
│       └── tag.rs          # Tag business logic
└── target/                 # Build artifacts (git-ignored)
```

## Docker Deployment

The project includes a multi-stage Dockerfile:

1. **Builder stage**: Rust Alpine image with build dependencies
2. **Production stage**: Minimal Alpine image with runtime dependencies only

```bash
# Build the Docker image
docker build -t axumbackend .

# Run the container
docker run -p 8000:8000 -e DATABASE_URL="..." axumbackend
```

The production image:
- Runs as non-root user (UID 1000)
- Strips the binary for smaller size
- Includes only runtime dependencies (libpq, ca-certificates)

## Data Models

### Post

```rust
pub struct Post {
    pub id: Uuid,
    pub title: String,
    pub body: Option<String>,
    pub created_by: Uuid,
    pub slug: String,
    pub photo_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub published: bool,
    pub view_count: i64,
    pub like_count: i64,
    pub bookmark_count: i64,
    pub user: User,
    pub tags: Vec<Tag>,
}
```

### User

```rust
pub struct User {
    pub id: Uuid,
    pub username: String,
}
```

### Tag

```rust
pub struct Tag {
    pub id: i32,
    pub name: String,
    pub created_at: Option<DateTime<Utc>>,
}
```

## Potential Improvements

- Add CRUD operations for posts (currently only GET endpoints)
- Implement authentication/authorization
- Add request body validation for POST/PUT endpoints
- Include structured logging with request IDs
- Add unit and integration tests
- Implement caching for frequently accessed data
- Add rate limiting middleware
- Implement proper health checks with database connectivity
