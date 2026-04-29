# AGENTS.md - Axum Backend

Single Rust binary (Axum 0.8.8, edition 2024, tokio-postgres, deadpool-postgres). No workspace, no DB migrations in repo.

## Build & Run

```bash
cargo run          # requires PostgreSQL and env vars
cargo build --release
```

No custom build scripts or task runners. No tests exist yet (`cargo test` compiles but runs 0 tests).

## Environment

`.env` is optional (`dotenvy::dotenv().ok()` in main.rs). The app has hardcoded defaults for every variable, so it can start without `.env` (but will fail to connect to DB if defaults are wrong).

Key vars:

```bash
PORT=8080                                       # default, not 8000
DATABASE_URL="postgresql://user:pass@host/db"   # or keyword/value format: host=localhost user=postgres ...
                                                  # default: host=localhost user=postgres password=postgres dbname=axumbackend
DB_POOL_MAX_SIZE=20
DB_POOL_CONNECTION_TIMEOUT=30
```

## Architecture

- **Router wiring**: `handlers/mod.rs` merges sub-routers with `.merge()` and applies `TraceLayer::new_for_http()` + `CorsLayer::permissive()` globally.
- **State**: `DbPool` (alias for `deadpool_postgres::Pool`) is passed via Axum `State<DbPool>`. Acquire client with `pool.get().await?`.
- **Routes**: All API routes under `/v1`. `health` handler mounts on `/` and `/health` (the README incorrectly lists `/v1/health`).
- **No DB migrations**: Schema is not tracked in this repo.

## API Patterns

- **Request validation**: Use `axum-valid` with `validator`. Always wrap extractors: `Valid(Query<T>)`, `Valid(Path<T>)` (see `handlers/post.rs`).
- **Path param regex**: Use `once_cell::sync::Lazy<Regex>` for `username`, `slug`, and `tag` validation.
- **Response wrapper**: Return `Json<ApiResponse<T>>`. Use `ApiResponse::success(data)` or `ApiResponse::with_meta(data, total, limit, offset)` for paginated lists.
- **Error response**: `AppError` implements `IntoResponse` as JSON: `{"success": false, "error": "...", "data": null}`. Variants: `Database`, `Pool`, `NotFound`, `BadRequest`, `InternalServerError`.
- **DB deserialization**: Models implement `From<&Row>`. `Post` has two constructors: `from` (truncates `body` to 200 chars) and `from_full` (does not truncate).

## Database Quirks

- **LIKE escaping**: `services/post.rs` manually escapes `\`, `%`, `_` before ILIKE queries and uses `ESCAPE '\\'` in SQL.
- **Order-by whitelist**: `validate_order_field` hardcodes allowed fields (`id`, `title`, `created_at`, `updated_at`, `view_count`, `like_count`, `bookmark_count`); defaults to `created_at`.
- **N+1 avoidance**: Batch-fetch tags with `ANY($1)` array parameter in `fetch_tags_for_posts`.

## Docker

Multi-stage Alpine build (`rust:1-alpine` → `alpine:3.19`). Build stage needs `musl-dev postgresql-dev`; runtime needs `libpq`. Exposes port 8080, runs as non-root user `app` (uid 1000).

## CI

`.github/workflows/docker-build.yml` only builds/pushes the Docker image on `main` branch pushes, tags, and PRs. No `cargo test` or `cargo clippy` checks in CI.

## Git

- Do not commit `.env` or `target/`.
- No force pushes to `main`.
