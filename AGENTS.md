# AGENTS.md

## Commands

```bash
cargo check              # fast type-check
cargo test                # run tests
cargo clippy              # lint
cargo fmt                 # format
cargo run                 # dev server (requires running PostgreSQL + .env)
```

Recommended order: `cargo fmt && cargo clippy && cargo test`

## Setup

- Copy `.env.example` to `.env` and configure `DATABASE_URL`. A running PostgreSQL instance is required.
- Config sections read via `XxxConfig::get()` are published once at startup by `Config::init_globals()` (`OnceLock`); never call `init` again after `main` runs.
- Config is loaded from env vars (via `dotenvy`), not from a config file. Primary keys are in `.env.example`; loaders and legacy aliases live in `src/config/loader.rs`. Use `parse_env` / `parse_env_alias` / `env_string` when adding a key.

## Architecture

**Stack**: Axum 0.8 + SeaORM 2.0 + Tokio + PostgreSQL. Edition 2024.

**Layered flow**: `handlers/` (HTTP, extraction, validation) → `services/` (business logic, DB queries via SeaORM) → `entities/` (SeaORM entity definitions) / `models/` (app-level DTOs and response types).

- `src/entities/` — SeaORM `DeriveEntityModel` structs mirroring DB tables. Do not confuse with `models/`.
- `src/models/` — Application-level types (response shapes, view models) that map from entity rows.
- `src/auth.rs` — `AuthUser` (JWT Bearer) and `AdminUser` (super-admin guard) Axum extractors.
- `src/response.rs` — `ApiResponse<T>` wrapper: `{ success, message, data, error, meta }`. Use `ApiResponse::error(message, error)` for failure envelopes.
- `src/middleware.rs` — security headers and CORS layer; routing itself stays in `handlers/mod.rs`.
- `src/error.rs` — `AppError` enum implementing `IntoResponse`; all errors flow through this.
- `src/rate_limit.rs` — Fixed-window rate limiter. In-memory by default; limiters built with `.shared(name)` (the auth routes) count in Redis when it is available.
- `src/cache.rs` — Optional, fail-open Redis/Valkey client (`cache::get()` returns `None` when `REDIS_URL` is empty or unreachable). Every caller must keep an in-memory/DB fallback. Keys are `<CACHE_KEY_PREFIX>:<parts>` and match echobackend's layout, so keep key names and cached JSON shapes compatible.
- `src/realtime.rs` — SSE fan-out hub; relays through Redis pub/sub (`<prefix>:realtime:<topic>`) when the cache is enabled.

## Conventions

- Every handler accepting input uses `VJson<T>` / `VQuery<T>` / `VPath<T>` (`src/extract.rs`) with `garde::Validate` derive. garde requires an attribute on every field (`#[garde(skip)]` when unvalidated); use `length(chars, ...)` for strings (default mode counts bytes) and `inner(custom(...))` for custom rules on `Option` fields. Custom rules return `garde::Error::new("<tag>")`, which becomes the `tag` in the 422 envelope.
- Error propagation uses `?`; services return `Result<_, DbErr>` or a domain error enum (e.g. `BookmarkError`). Map a domain error to HTTP with `impl From<XError> for AppError` in the matching handler file, so handlers just use `?`. (Exceptions: `auth` takes a context message, and `guild` has separate guild/channel mappings.)
- API responses always use `ApiResponse::success_with_message` or `ApiResponse::with_meta_message` for paginated data.
- Route registration uses `Router::merge` per domain in `handlers/mod.rs`.
- No migrations in this repo — manage DB schema externally. Entity files must stay in sync with the actual schema.

## Docker

Multi-stage Debian build using `cargo-chef` for dependency caching (`rust:1.98-trixie` → `debian:trixie-slim`). Rust version is pinned for reproducibility. Dependencies build in a cached layer; source changes don't invalidate dependency cache. Production image runs as non-root user with healthcheck on `/health` (curl). Pushes to `cecep31/axumbackend` on Docker Hub via CI (`.github/workflows/docker-build.yml`). Only triggers on `main` branch pushes and version tags.

## Known Gotchas

- `DbPool` is a type alias for `DatabaseConnection` (SeaORM), not a separate connection pool library.
- Pagination query params differ per endpoint (some use `PaginationQuery`, others use `limit`/`offset` directly); check individual handlers.
