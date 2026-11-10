# Axum Backend

Rust backend (Axum + SeaORM + PostgreSQL) for the Pilput platform. It is a port of the Go `echobackend` and keeps its API behaviour: response envelope, validation errors, rate limits and Redis key layout.

## Features

- **Auth**: register/login, rotating refresh tokens with sessions, password reset by email, activity logs, GitHub OAuth (one-time exchange code). Passwords use Argon2id; legacy bcrypt hashes are upgraded on login.
- **Blog**: posts, tags, comments, likes, bookmarks (with folders), views, follows, reports and moderation, notifications, statistics.
- **Guilds**: communities with members, channels and messages, with realtime delivery over SSE.
- **AI chat**: conversations and messages via OpenRouter, with SSE streaming and generated titles.
- **AI article generation**: streams an HTML draft article (SSE) from a prompt, tone and length for the create/edit-post page; nothing is stored.
- **Holdings**: investment portfolio with analytics, corporate actions, and stock/exchange-rate quotes.
- **Platform**: global and per-route rate limiting, optional Redis cache (fail-open), S3 uploads, SMTP/Resend email, structured logging, graceful shutdown, Docker.

## Quick Start

### 1. Set up environment

```bash
cp .env.example .env
```

Edit `.env` with your database credentials:

```env
PORT=8080
DATABASE_URL="postgresql://postgres:password@localhost:5432/axumbackend"
DB_POOL_MAX_OPEN=5
JWT_SECRET="change-this-to-at-least-32-characters"
FRONTEND_RESET_PASSWORD_URL="http://localhost:3000/reset-password"
FRONTEND_DELETE_ACCOUNT_URL="http://localhost:3000/confirm-delete-account"
SMTP_HOST=""
SMTP_FROM="noreply@pilput.net"
```

### 2. Run the application

```bash
cargo run
```

Server starts on `http://localhost:8080`

## API Endpoints

All routes are under `/api` unless noted. Responses use the `{ success, message, data, error, meta }` envelope. `Auth` = Bearer JWT; `Admin` = super admin (checked in the DB).

| Area | Endpoints |
|------|-----------|
| Health | `GET /`, `GET /health` |
| Auth | `POST /auth/register`, `/auth/login`, `/auth/refresh`, `/auth/logout`, `/auth/forgot-password`, `/auth/reset-password`, `/auth/check-username`; `GET/PUT /auth/profile`; `PATCH /auth/password`; `POST /auth/account/delete-request`, `/auth/account/delete-confirm`; `POST /auth/sessions/revoke-all`; `DELETE /auth/sessions/{user_id}` (Admin); `GET /auth/activity-logs[/recent\|/failed-logins]` |
| GitHub OAuth | `GET /auth/oauth/github`, `GET /auth/oauth/github/callback`, `POST /auth/oauth/exchange` |
| Posts | `GET/POST /posts`; `GET/PUT/DELETE /posts/{id}`; `GET /posts/random`, `/trending`, `/sitemap`, `/me`, `/feed/for-you`, `/tag/{tag}`, `/username/{username}`, `/u/{username}/{slug}`; `GET/PUT/DELETE /posts/me/{id}`; `POST /posts/image`; stats: `/posts/stats`, `/stats/engagement`, `/me/analytics[/likes-by-month]` |
| Post engagement | `POST/DELETE /posts/{id}/like`, `GET /posts/{id}/likes\|like-stats\|liked`; `POST /posts/{id}/view`, `GET /posts/{id}/views\|view-stats\|viewed` |
| Moderation | `POST /posts/{id}/reports`, `GET /posts/reports`, `POST/GET /posts/{id}/moderation` |
| Comments | `GET/POST /posts/{id}/comments`, `PUT/DELETE /posts/{id}/comments/{comment_id}` |
| Tags | `GET/POST /tags`, `GET/PUT/DELETE /tags/{id}`, `GET /tags/stats\|trending\|sitemap` |
| Users | `GET/POST /users`, `GET/PUT/DELETE /users/{id}`, `GET /users/me`, `/users/stats`, `/users/username/{username}`, `POST /users/me/image`, `POST /users/{id}/restore`; follows: `POST /users/follow`, `DELETE /users/{id}/follow`, `GET /users/{id}/followers\|following\|follow-stats\|follow-status\|mutual-follows` |
| Bookmarks | `GET /bookmarks`, `POST/PATCH /bookmarks/{id}`, `PATCH /bookmarks/{id}/move`, `GET/POST /bookmarks/folders`, `PATCH/DELETE /bookmarks/folders/{folder_id}` |
| Notifications | `GET /notifications`, `GET /notifications/unread-count`, `PATCH /notifications/read-all`, `PATCH /notifications/{id}/read` |
| Guilds | `GET/POST /guilds`, `GET /guilds/me`, `GET/PATCH/DELETE /guilds/{slug}`, `GET /guilds/{slug}/members`, `POST /guilds/{slug}/join`, `DELETE /guilds/{slug}/leave`, channels and messages under `/guilds/{slug}/channels/...` (plus an SSE stream) |
| AI chat | `GET/POST /chat/conversations`, `POST /chat/conversations/stream`, `GET/PUT/DELETE /chat/conversations/{id}`, `GET/POST /chat/conversations/{id}/messages`, `POST .../messages/stream`, `GET/DELETE /chat/messages/{id}` |
| AI article | `POST /ai/generate-article` (SSE; super admins only, 20/hour; 503 when OpenRouter is not configured) |
| Holdings | `GET/POST /holdings`, `GET/PUT/DELETE /holdings/{id}`, `GET /holdings/summary\|trends\|compare\|monthly\|calendar`, `POST /holdings/duplicate\|sync`, `GET /holding-types` |
| Exchange rates | `GET /exchange-rates` |

The routers in `src/handlers/` are the source of truth for the exact list.

### Password Reset and Account Deletion Email

`POST /api/auth/forgot-password` always returns the same success response for registered and unregistered emails. Like echobackend, the email is queued in-process (capacity 128) and delivered by 2 background workers, so the request never waits on delivery:

| Variable | Description |
|----------|-------------|
| `SMTP_HOST` / `SMTP_PORT` | SMTP server (delivery is enabled when `SMTP_HOST` is set) |
| `SMTP_USERNAME` / `SMTP_PASSWORD` | Optional credentials (PLAIN auth) |
| `SMTP_TLS` | `true` for implicit TLS (port 465); otherwise STARTTLS when offered |
| `SMTP_TIMEOUT_SECONDS` / `SMTP_TASK_TIMEOUT_SECONDS` | Per-operation and per-email timeouts |
| `SMTP_FROM` (alias `EMAIL_FROM`) | Sender email address |
| `RESEND_API_KEY` | axum-only fallback transport, used when `SMTP_HOST` is empty |
| `FRONTEND_RESET_PASSWORD_URL` | Frontend reset page; backend appends `?token=...` |
| `FRONTEND_DELETE_ACCOUNT_URL` | Frontend page confirming an account deletion (default `http://localhost:3000/confirm-delete-account`); backend appends `?token=...` |

If no transport is configured, the reset token is still created and a `devMode: true` activity log is recorded (the link itself is never logged).

Deleting an account needs email confirmation: `POST /api/auth/account/delete-request` (authenticated, 3 per 5 min) emails a one-time link valid for 30 minutes, and `POST /api/auth/account/delete-confirm` with `{"token": "..."}` (no auth, 5 per 5 min) soft-deletes the account and revokes its sessions. Without a configured transport the request answers 503 and creates no token. Tokens live in `account_deletion_tokens` (echobackend migration `20261109120000`).

## Development

```bash
# Build
cargo build

# Run
cargo run

# Test
cargo test

# Format
cargo fmt

# Lint
cargo clippy
```

## Docker

```bash
docker build -t axumbackend .
docker run -p 8080:8080 --env-file .env axumbackend
```

## Project Structure

```
src/
├── main.rs         # Entry point
├── state.rs        # Shared AppState
├── config/         # Env-based configuration (env.rs parsing helpers, loader.rs per-section loaders, types.rs)
├── handlers/       # Routes + HTTP handlers (per domain; large ones are folders: auth/, post/)
├── services/       # Business logic (large ones are folders: auth/, post/, holding/, chat/)
├── dto/            # Request/query input types
├── models/         # Response types
├── entities/       # SeaORM entities
├── http/           # Extractors, responses, errors, middleware, rate limit
├── infra/          # Database, cache, storage, email, realtime
├── clients/        # Third-party API clients
└── domain/         # Slug, antispam
```

The database schema is managed outside this repo (no migrations); entities must match it.

## Tech Stack

- **Framework**: Axum 0.8
- **Database**: PostgreSQL via SeaORM 2.0 (sqlx)
- **Async runtime**: Tokio
- **Serialization**: Serde
- **Validation**: garde
- **Cache / pub-sub**: Redis or Valkey (optional)
- **Storage / email**: S3-compatible (rusty-s3), SMTP (lettre) or Resend
