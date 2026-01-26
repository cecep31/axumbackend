# Axum Backend Project

## Project Overview

This is a Rust-based web backend application built with the Axum web framework. The project implements a blog post management system with PostgreSQL database integration. Key features include:

- REST API endpoints for health checking and retrieving blog posts
- PostgreSQL database for data persistence
- Structured architecture with separation of concerns (models, routes, services, database layer)
- Built with Rust 2024 edition

### Architecture

The project follows a modular architecture with the following components:

- **Main Application (`src/main.rs`)**: Initializes the Axum web server, establishes database connection, and mounts routes
- **Configuration (`src/config.rs`)**: Handles application configuration from environment variables
- **Database Layer (`src/database.rs`)**: Handles PostgreSQL database connection using tokio-postgres
- **Error Handling (`src/error.rs`)**: Defines custom error types and their HTTP response mapping
- **Models (`src/models/`)**: Defines data structures (Post, User, Tag, Response)
- **Routes (`src/handlers/`)**: Contains HTTP endpoint handlers
- **Services (`src/services/`)**: Implements business logic for data operations

### Dependencies

- `axum`: Web framework for routing and HTTP handling
- `tokio-postgres`: PostgreSQL database client
- `serde`: Serialization/deserialization framework
- `chrono`: Date/time handling with UTC support
- `uuid`: UUID generation and handling
- `tower-http`: HTTP middleware including CORS support
- `dotenvy`: Environment variable loading

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

# Run tests (if any exist)
cargo test
```

### Environment Variables
Create a `.env` file in the project root with the following variables:
```
DATABASE_URL=host=localhost user=postgres password=postgres dbname=axumbackend
PORT=8000
```

### Running the Application
The application connects to a PostgreSQL database specified by the DATABASE_URL environment variable. The server listens on the port specified by the PORT environment variable (defaults to 8000) and exposes the following endpoints:

- `GET /` and `GET /v1/health`: Health check endpoints returning status information
- `GET /v1/posts`: Retrieves all blog posts from the database
- `GET /v1/posts/random?limit=N`: Retrieves random blog posts with an optional limit parameter
- `GET /v1/posts/tag/{tag}`: Retrieves blog posts filtered by a specific tag
- `GET /v1/posts/u/{username}/{slug}`: Retrieves a specific blog post by username and slug

## Development Conventions

### Code Structure
- Modules are organized by concern (models, routes, services, database)
- Each module has its own file or directory
- Route handlers delegate business logic to service functions
- Database connections are managed through Axum's state system using a connection pool

### Data Model
The `Post` model includes:
- `id`: UUID primary key
- `title`: String representing the post title
- `body`: String containing the post content (truncated to 200 chars in some queries)
- `created_by`: UUID referencing the user who created the post
- `slug`: URL-friendly identifier for the post
- `photo_url`: Optional URL for a post image
- `created_at`: DateTime in UTC format
- `updated_at`: DateTime in UTC format
- `deleted_at`: Optional DateTime for soft deletion
- `published`: Boolean indicating if the post is published
- `view_count`: Number of views
- `like_count`: Number of likes
- `user`: Associated User object
- `tags`: List of associated Tag objects

The `User` model includes:
- `id`: UUID primary key
- `username`: String representing the user's name

The `Tag` model includes:
- `id`: UUID primary key
- `name`: String representing the tag name
- `created_at`: DateTime in UTC format

### Response Format
API responses follow a consistent structure using `ApiResponse<T>`:
- `success`: Boolean indicating if the request was successful
- `data`: Optional field containing the response data
- `meta`: Metadata including total count, limit, offset, and total pages for pagination

## File Structure
```
axumbackend/
├── Cargo.toml          # Project manifest and dependencies
├── Cargo.lock          # Dependency lock file
├── Dockerfile          # Docker configuration for building and deploying
├── .dockerignore       # Files to ignore during Docker build
├── .gitignore          # Files to ignore by Git
├── GEMINI.md           # Gemini-specific documentation
├── QWEN.md             # Qwen-specific documentation
├── AGENTS.md           # Documentation for AI agents
├── .env.example        # Example environment variables
├── src/
│   ├── main.rs         # Application entry point
│   ├── config.rs       # Configuration from environment variables
│   ├── database.rs     # Database connection and setup
│   ├── error.rs        # Custom error types and HTTP responses
│   ├── response.rs     # API response wrapper
│   ├── models/
│   │   ├── mod.rs      # Models module declaration
│   │   ├── post.rs     # Post data model
│   │   ├── user.rs     # User data model
│   │   └── tag.rs      # Tag data model
│   ├── handlers/
│   │   ├── mod.rs      # Handlers module declaration
│   │   ├── health.rs   # Health check endpoint
│   │   ├── post.rs     # Posts endpoints
│   │   └── tag.rs      # Tags endpoints
│   └── services/
│       ├── mod.rs      # Services module declaration
│       ├── post.rs     # Posts business logic
│       └── tag.rs      # Tags business logic
└── target/             # Build artifacts (generated)
```

## Docker Deployment

The project includes a multi-stage Dockerfile that:
1. Builds the application in a Rust Alpine container
2. Creates a minimal production image with only necessary runtime dependencies
3. Runs the application as a non-root user for security

To build and run with Docker:
```bash
# Build the Docker image
docker build -t axumbackend .

# Run the container
docker run -p 8000:8000 -e DATABASE_URL="..." axumbackend
```

## Potential Improvements

- Add more comprehensive error handling instead of using `unwrap_or_else`
- Implement CRUD operations for posts (currently only GET is implemented)
- Add request validation and sanitization
- Include logging capabilities
- Add unit and integration tests
- Implement proper date parsing with error handling