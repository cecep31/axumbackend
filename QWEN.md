# Rocket Backend Project

## Project Overview

This is a Rust-based web backend application built with the Rocket web framework. The project implements a simple blog post management system with SQLite database integration. Key features include:

- REST API endpoints for health checking and retrieving blog posts
- SQLite database for data persistence
- Structured architecture with separation of concerns (models, routes, services, database layer)
- Built with Rust 2024 edition

### Architecture

The project follows a modular architecture with the following components:

- **Main Application (`src/main.rs`)**: Initializes the Rocket web server, establishes database connection, and mounts routes
- **Database Layer (`src/database.rs`)**: Handles SQLite database connection and creates the `posts` table schema
- **Models (`src/models/`)**: Defines data structures (currently only `Post`)
- **Routes (`src/routes/`)**: Contains HTTP endpoint handlers
- **Services (`src/services/`)**: Implements business logic for data operations

### Dependencies

- `rocket`: Web framework for routing and HTTP handling
- `rusqlite`: SQLite database client with bundled support
- `serde`: Serialization/deserialization framework
- `chrono`: Date/time handling with UTC support
- `parking_lot`: Advanced synchronization primitives (Mutex)

## Building and Running

### Prerequisites
- Rust toolchain (edition 2024)
- Cargo package manager

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

### Running the Application
The application will create a `database.db` SQLite file in the project root when started. The server listens on the default Rocket port (typically 8000) and exposes the following endpoints:

- `GET /health`: Health check endpoint returning "OK"
- `GET /posts`: Retrieves all blog posts from the database

## Development Conventions

### Code Structure
- Modules are organized by concern (models, routes, services, database)
- Each module has its own file or directory
- Route handlers delegate business logic to service functions
- Database connections are managed through Rocket's state system using a Mutex-wrapped Connection

### Data Model
The `Post` model includes:
- `id`: Integer primary key
- `title`: String representing the post title
- `body`: String containing the post content
- `published_at`: DateTime in UTC format

### Error Handling
The application uses Rust's Result type for error handling, with some unwrapping in the route handlers that could be improved for production use.

## File Structure
```
rocketbackend/
├── Cargo.toml          # Project manifest and dependencies
├── Cargo.lock          # Dependency lock file
├── database.db         # SQLite database file (generated)
├── GEMINI.md           # Gemini-specific documentation
├── QWEN.md             # Qwen-specific documentation
├── src/
│   ├── main.rs         # Application entry point
│   ├── database.rs     # Database connection and setup
│   ├── models/
│   │   ├── mod.rs      # Models module declaration
│   │   └── post.rs     # Post data model
│   ├── routes/
│   │   ├── mod.rs      # Routes module declaration
│   │   ├── health.rs   # Health check endpoint
│   │   └── post.rs     # Posts endpoints
│   └── services/
│       ├── mod.rs      # Services module declaration
│       └── post.rs     # Posts business logic
└── target/             # Build artifacts (generated)
```

## Potential Improvements

- Add more comprehensive error handling instead of using `unwrap_or_else`
- Implement CRUD operations for posts (currently only GET is implemented)
- Add request validation and sanitization
- Include logging capabilities
- Add unit and integration tests
- Implement proper date parsing with error handling