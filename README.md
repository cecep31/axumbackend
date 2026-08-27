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
DB_POOL_MAX_SIZE=20
JWT_SECRET="change-this"
FRONTEND_RESET_PASSWORD_URL="http://localhost:3000/reset-password"
RESEND_API_KEY=""
EMAIL_FROM="noreply@pilput.net"
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

`POST /api/auth/forgot-password` always returns the same success response for registered and unregistered emails. When `RESEND_API_KEY` is configured, the backend sends the reset link through Resend using:

| Variable | Description |
|----------|-------------|
| `RESEND_API_KEY` | Resend API key for email delivery |
| `EMAIL_FROM` | Sender email address |
| `FRONTEND_RESET_PASSWORD_URL` | Frontend reset page; backend appends `?token=...` |

If Resend is not configured, the reset token is still created and the reset link is recorded in auth activity metadata for local development.

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
├── config.rs       # Configuration
├── database.rs     # Database setup
├── error.rs        # Error handling
├── response.rs     # API responses
├── models/         # Data models
├── handlers/       # HTTP handlers
└── services/       # Business logic
```

## Tech Stack

- **Framework**: Axum 0.8
- **Database**: PostgreSQL + deadpool-postgres
- **Async**: Tokio
- **Serialization**: Serde
- **Validation**: axum-valid
