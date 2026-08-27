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
- `JwtConfig` is initialized once at startup via `OnceLock`; do not attempt to set it again after `main` runs.
- Config is loaded from env vars (via `dotenvy`), not from a config file. All env keys are documented in `src/config.rs`.

## Architecture

**Stack**: Axum 0.8 + SeaORM 2.0 + Tokio + PostgreSQL. Edition 2024.

**Layered flow**: `handlers/` (HTTP, extraction, validation) → `services/` (business logic, DB queries via SeaORM) → `entities/` (SeaORM entity definitions) / `models/` (app-level DTOs and response types).

- `src/entities/` — SeaORM `DeriveEntityModel` structs mirroring DB tables. Do not confuse with `models/`.
- `src/models/` — Application-level types (response shapes, view models) that map from entity rows.
- `src/auth.rs` — `AuthUser` (JWT Bearer) and `AdminUser` (super-admin guard) Axum extractors.
- `src/response.rs` — `ApiResponse<T>` wrapper: `{ success, message, data, error, meta }`.
- `src/error.rs` — `AppError` enum implementing `IntoResponse`; all errors flow through this.
- `src/rate_limit.rs` — In-memory rate limiter (not Redis-backed).

## Conventions

- Every handler accepting input uses `Valid<Json<T>>` or `Valid<Query<T>>` with `validator::Validate` derive.
- Error propagation uses `?`; services return `Result<_, DbErr>` or `Result<_, AppError>`.
- API responses always use `ApiResponse::success_with_message` or `ApiResponse::with_meta_message` for paginated data.
- Route registration uses `Router::merge` per domain in `handlers/mod.rs`.
- No migrations in this repo — manage DB schema externally. Entity files must stay in sync with the actual schema.

## Docker

Multi-stage Debian build (`rust:1-trixie` → `debian:trixie-slim`). Pushes to `cecep31/axumbackend` on Docker Hub via CI (`.github/workflows/docker-build.yml`). Only triggers on `main` branch pushes and version tags.

## Known Gotchas

- `DbPool` is a type alias for `DatabaseConnection` (SeaORM), not a separate connection pool library.
- `config.rs` doc comments mention `DB_POOL_MAX_LIFETIME` and `DB_POOL_IDLE_TIMEOUT` env vars, but they are **not implemented** — only `DB_POOL_MAX_SIZE` and `DB_POOL_CONNECTION_TIMEOUT` are actually parsed.
- Pagination query params differ per endpoint (some use `PaginationQuery`, others use `limit`/`offset` directly); check individual handlers.
