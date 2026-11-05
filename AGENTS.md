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
- Config is loaded from env vars (via `dotenvy`), not from a config file. Primary keys are in `.env.example`; parsing helpers live in `src/config/env.rs`, per-section loaders and legacy aliases in `src/config/loader.rs`. Use `parse_env` / `parse_env_alias` / `parse_bool_alias` / `parse_duration_alias` / `env_string` when adding a key. They follow echobackend's `config/env.go`: the first set alias wins, an unparsable value falls back to the default (never panics), bools use Go `strconv.ParseBool`, durations use Go `time.ParseDuration` (`parse_go_duration`).

## Architecture

**Stack**: Axum 0.8 + SeaORM 2.0 + Tokio + PostgreSQL. Edition 2024.

**Layered flow**: `handlers/` (HTTP, extraction, validation) → `services/` (business logic, DB queries via SeaORM) → `entities/` (SeaORM entity definitions) / `models/` (response types).

```
src/
├── handlers/   # routes + HTTP handlers, one module per domain (large domains are folders, e.g. handlers/post/)
├── services/   # business logic; large services are folders that re-export their public API from mod.rs
├── dto/        # INPUT only: request bodies, query/path params (garde-validated)
├── models/     # OUTPUT only: response shapes / view models mapped from entity rows
├── entities/   # SeaORM entities mirroring DB tables
├── http/       # HTTP plumbing: auth extractors, extract, response, error, middleware, rate_limit, upload
├── infra/      # infrastructure: database, cache, storage (S3), email, realtime
├── clients/    # third-party API clients without their own endpoint (openrouter, market_quote)
├── domain/     # pure domain helpers: slug, antispam
└── config/
```

- `dto/` vs `models/`: request/query types go in `dto/`, anything serialized back to the client goes in `models/`. Services may take a request DTO as input but return `models/` types.
- `src/entities/` — SeaORM `DeriveEntityModel` structs mirroring DB tables. Do not confuse with `models/`.
- `src/http/auth.rs` — `AuthUser` (JWT Bearer), `OptionalAuthUser` and `AdminUser` extractors. `AdminUser` reads `users.is_super_admin` from the DB (never the JWT claim; 401 "Failed to validate privileges" / 403 "Insufficient privileges"); `http::auth::is_super_admin(db, id)` is the in-handler check (false on error). Auth business logic lives in `services/auth/`.
- `src/http/client_ip.rs` — `ClientIp` / `RequestClient` extractors (echobackend `c.RealIP()` honouring `HTTP_TRUST_PROXY`, plus User-Agent as `domain::client::ClientInfo`). Never read `X-Forwarded-For` directly.
- `src/http/response.rs` — `ApiResponse<T>` wrapper: `{ success, message, data, error, meta }`. Use `ApiResponse::message(msg)` for payload-less success (no `data` key), `ApiResponse::error(message, error)` / `failure(message, None)` for failure envelopes.
- `src/http/middleware.rs` — security headers (HSTS `max-age=3600; includeSubdomains` only when a trusted loopback/private/link-local peer forwards https, Echo `c.Scheme()`), CORS (echobackend defaults, no credentials), the 404/405 envelope fallbacks (unknown paths under echobackend's `Auth()` groups run the `AuthUser` check first), the 408 envelope request timeout and the panic handler (`CatchPanicLayer`, 500 "Internal server error"); routing itself stays in `handlers/mod.rs`.
- `src/http/error.rs` — `AppError` enum implementing `IntoResponse`; all errors flow through this. Rendering follows echobackend `pkg/response`: 401 `error` is always "Unauthorized access", 403 "Access forbidden", 500s never carry `error`. `AppError::context(msg)` reproduces echobackend's `respondError(c, msg, err)` (handler message, domain detail kept as `error`). `AppError::payload_too_large()` is the global 413 envelope, `AppError::status_text(status)` echobackend's `HTTPStatus` for a plain 4xx.
- `src/http/rate_limit.rs` — `TokenBucketLimiter` (global `HTTP_RATE_LIMIT_RPS`, per client IP, burst 2x, inside CORS) and fixed-window per-route `RateLimiter`s; limiters built with `.shared(name)` (the auth routes) count in Redis when it is available and per client only.
- `src/infra/cache.rs` — Optional, fail-open Redis/Valkey client (`cache::get()` returns `None` when `REDIS_URL` is empty or unreachable). Every caller must keep an in-memory/DB fallback. Keys are `<CACHE_KEY_PREFIX>:<parts>` and match echobackend's layout, so keep key names and cached JSON shapes compatible.
- `src/infra/realtime.rs` — SSE fan-out hub; relays through Redis pub/sub (`<prefix>:realtime:<topic>`) when the cache is enabled.

## Conventions

- Every handler accepting input uses `VJson<T>` / `VQuery<T>` / `VPath<T>` (`src/http/extract.rs`) with `garde::Validate` derive. garde requires an attribute on every field (`#[garde(skip)]` when unvalidated); use `length(chars, ...)` for strings (default mode counts bytes) and `inner(custom(...))` for custom rules on `Option` fields. Custom rules return `garde::Error::new("<tag>")`, which becomes the `tag` in the 422 envelope. For echobackend handlers that bind without validating (`c.Bind` only, e.g. `PUT /api/holdings/{id}`), use `PJson<T>` (400 "Invalid request format" on a bad body). For echobackend `c.Bind` + `c.Validate` handlers answering "Invalid request format" (auth bodies, admin user create/update), use `VBind<T>`: an empty body is `T::default()` then validated, a bad body is 400 "Invalid request format". `VJson` keeps "Invalid request body" (e.g. `POST /api/users/follow`). When echobackend answers a bind failure with its own message ("Failed to create post", "Invalid request payload"), take the body as `Result<VBind<T>, AppError>` and map it with `http::extract::bind_message(msg)` (rewrites only the 400; 413/422 untouched).
- echobackend `validate:"required,..."` on a string: `#[serde(default)]` on the struct/field plus `#[garde(custom(dto::validation::required), ...)]` (custom rules run first). The 422 envelope keeps only the first error per field and orders fields by declaration (`validate_input`), like go-playground.
- Canonical UUID checks (echobackend `validator.IsValidUUID`) use `dto::validation::{is_canonical_uuid, parse_canonical_uuid}`; `/api/users/{id}` routes take `handlers::user::UserIdParam` (400 "Invalid user ID" / "invalid user ID format"), declared after the auth extractor. Other id params follow the same pattern with echobackend's per-route message, via `http::extract::raw_path_param`: `handlers::post::{PostIdParam, LikePostIdParam}` ("Invalid post ID" / "invalid post ID format"), `handlers::comment::CommentIdsParam`, `handlers::tag::TagIdParam` (Go `ParseUint(.., 10, 32)`), `handlers::notification::NotificationIdParam`, `handlers::chat::{ConversationIdParam, MessageIdParam}` (UUID segments via `http::extract::canonical_uuid_param`), `handlers::holding::HoldingIdParam` (Go `ParseInt(.., 10, 64)`, `dto::holding::parse_holding_id`). Guild channel/message ids and the `?before=` cursor are parsed with `parse_canonical_uuid` too. Numeric `required` uses `dto::validation::required_num`.
- Error propagation uses `?`; services return `Result<_, DbErr>` or a domain error enum (e.g. `BookmarkError`). Map a domain error to HTTP with `impl From<XError> for AppError` in the matching handler file, so handlers just use `?`. (Exceptions, mirroring echobackend's per-endpoint messages: `auth` takes a context message; `chat_err(msg)`, `holding_err(msg)`, and `guild`'s `map_guild_error(msg)` / `map_guild_channel_error(msg)` are closures taking the endpoint message used for 500s and, where echobackend does, as the 404/400 `message`.)
- API responses always use `ApiResponse::success_with_message` or `ApiResponse::with_meta_message` for paginated data.
- Route registration uses `Router::merge` per domain in `handlers/mod.rs`.
- No migrations in this repo — manage DB schema externally. Entity files must stay in sync with the actual schema.

## Docker

Multi-stage Debian build using `cargo-chef` for dependency caching (`rust:1.98-trixie` → `debian:trixie-slim`). Rust version is pinned for reproducibility. Dependencies build in a cached layer; source changes don't invalidate dependency cache. Production image runs as non-root user with healthcheck on `/health` (curl). Pushes to `cecep31/axumbackend` on Docker Hub via CI (`.github/workflows/docker-build.yml`). Only triggers on `main` branch pushes and version tags.

## Known Gotchas

- `DbPool` is a type alias for `DatabaseConnection` (SeaORM), not a separate connection pool library.
- Pagination is lenient like echobackend's `ParsePaginationParams`: keep `limit`/`offset` as `Option<String>` with `#[garde(skip)]` and resolve them with `dto::validation::parse_pagination` (or `PaginationQuery::resolve`); never reject them with 400/422.
- 422 `errors[]` items are `{field, message, value?, tag}` with `field` as the Go struct field name (`first_name` -> `FirstName`, `user_id` -> `UserID`), matching echobackend's validator.
