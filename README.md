# Axum Backend

A simple Rust web backend built with Axum and PostgreSQL for blog post management.

## Features

- REST API for blog posts
- PostgreSQL database with connection pooling
- Input validation
- Structured logging
- Docker support

## Quick Start

### 1. Set up environment

```bash
cp .env.example .env
```

Edit `.env` with your database credentials:

```env
PORT=8080
DATABASE_URL="postgresql://postgres:password@localhost:5432/axumbackend"
DB_POOL_MAX_OPEN=20
JWT_SECRET="change-this-to-at-least-32-characters"
FRONTEND_RESET_PASSWORD_URL="http://localhost:3000/reset-password"
SMTP_HOST=""
SMTP_FROM="noreply@pilput.net"
```

### 2. Run the application

```bash
cargo run
```

Server starts on `http://localhost:8080`

## API Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/health` | Health check |
| POST | `/api/auth/register` | Register a user |
| POST | `/api/auth/login` | Login with email or username |
| POST | `/api/auth/forgot-password` | Request a password reset link |
| POST | `/api/auth/reset-password` | Set a new password with a reset token |
| POST | `/api/auth/refresh` | Rotate access and refresh tokens |
| POST | `/api/auth/logout` | Logout and delete a refresh session |
| GET | `/api/auth/profile` | Get current user profile |
| PATCH | `/api/auth/password` | Change password |
| GET | `/api/auth/activity-logs` | Get current user auth activity |
| GET | `/api/auth/oauth/github` | Redirect to GitHub authorization (sets `github_oauth_state` cookie) |
| GET | `/api/auth/oauth/github/callback` | GitHub OAuth callback; redirects to frontend with one-time `code` |
| POST | `/api/auth/oauth/exchange` | Exchange one-time OAuth code for access/refresh tokens |
| GET | `/api/posts` | Get all posts |
| GET | `/api/posts/random?limit=N` | Get random posts |
| GET | `/api/posts/tag/{tag}` | Get posts by tag |
| GET | `/api/posts/u/{username}/{slug}` | Get post by author |

### Password Reset Email

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

If no transport is configured, the reset token is still created and a `devMode: true` activity log is recorded (the link itself is never logged).

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
├── config/         # Env-based configuration
├── handlers/       # Routes + HTTP handlers (per domain)
├── services/       # Business logic
├── dto/            # Request/query input types
├── models/         # Response types
├── entities/       # SeaORM entities
├── http/           # Extractors, responses, errors, middleware, rate limit
├── infra/          # Database, cache, storage, email, realtime
├── clients/        # Third-party API clients
└── domain/         # Slug, antispam
```

## Tech Stack

- **Framework**: Axum 0.8
- **Database**: PostgreSQL + deadpool-postgres
- **Async**: Tokio
- **Serialization**: Serde
- **Validation**: axum-valid
