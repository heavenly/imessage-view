# AGENTS.md — imessage-db-port

Rust/Axum web application that imports macOS iMessage data into a local SQLite database and serves a browsable UI with search, analytics, and attachment recovery.

## Build / Run / Test Commands

```bash
cargo build                              # compile (debug)
cargo build --release                    # compile (release)
cargo run -- import                      # import iMessage data (incremental)
cargo run -- import --full               # full reimport (drops + recreates tables)
cargo run -- serve                       # start web server on 0.0.0.0:3000
cargo run -- scan-ios-backup --backup-path <dir> [--copy]

cargo test                               # run all tests
cargo test <test_name>                   # run a single test by name
cargo test --lib db::tests               # run tests in a specific module
cargo test -- --nocapture                # show println output during tests

cargo clippy                             # lint
cargo fmt                                # format
cargo fmt -- --check                     # check formatting without modifying
```

## Architecture

```
src/
  main.rs          — CLI (clap), entry point, subcommand dispatch
  state.rs         — AppState: Arc<Mutex<Connection>> + attachment_root
  error.rs         — Minimal unit Error type for the import layer
  db/
    mod.rs         — create_db, set_pragmas, drop_and_recreate
    schema.rs      — DDL constants (CREATE TABLE, CREATE INDEX)
    queries.rs     — All SQL query functions (read/write)
  import/
    mod.rs         — run_import orchestration
    messages.rs    — Message import from source iMessage DB
    attachments.rs — Attachment import
    contacts.rs    — Contact resolution from AddressBook
  web/
    mod.rs         — Axum router definition
    pages.rs       — Full-page handlers (index, conversation, analytics, etc.)
    partials.rs    — HTMX partial-response handlers
    attachments.rs — Attachment serving, thumbnails, downloads
    recovery.rs    — iOS backup recovery UI
  search/mod.rs    — FTS5 + LIKE search logic
  recovery/
    mod.rs
    ios_backup.rs  — iOS backup scanning and file recovery
  models/mod.rs    — Shared data model structs
templates/         — Askama HTML templates (base.html + pages + partials/)
static/            — CSS and JS assets
```

Data flow: macOS iMessage DB → import layer → local SQLite (`data/imessage.db`) → web layer serves via Axum.

## Code Style

### Imports

Order: external crates and `std` (intermixed, alphabetical), then blank line, then `crate::`/`super::` imports.

```rust
use askama::Template;
use axum::extract::{Query, State};
use chrono::DateTime;
use std::path::PathBuf;

use crate::db::queries;
use crate::state::AppState;
use super::pages::ConversationRow;
```

- Use nested braces for multiple items: `use std::sync::{Arc, Mutex};`
- No wildcard imports except `use super::*;` in `#[cfg(test)]` blocks.
- Multi-line nested imports for large crate trees (e.g., `imessage_database`).

### Naming

- Functions/variables: `snake_case` — `import_messages`, `build_conversation_rows`
- Types/structs/enums: `PascalCase` — `AppState`, `ConversationListRow`
- Enum variants: `PascalCase` — `Commands::ScanIosBackup`
- Constants: `SCREAMING_SNAKE_CASE` — `MESSAGES_PER_PAGE`, `CREATE_CONTACTS`
- Booleans: `is_` prefix (`is_group`, `is_from_me`) or `has_` prefix (`has_attachments`, `has_photo`)
- Common abbreviations: `conn`, `stmt`, `tx`, `pb`, `dst`, `src`, `att`, `fmt`

### Error Handling

Two strategies coexist:

1. **`anyhow::Result<T>`** — used in `db/`, `web/`, and `main.rs`. Propagate with `?`, bail with `anyhow::bail!()`.
2. **`crate::error::Result<T>`** — used in `import/`. A unit `Error` struct that discards details; callers log via `eprintln!` and wrap with `anyhow::anyhow!()`.

Web handlers swallow errors with `.unwrap_or_default()` — they never return error types to the client, except `web/attachments.rs` which returns `Result<impl IntoResponse, StatusCode>`.

`state.db.lock().unwrap()` is the standard Mutex access pattern. Use `.unwrap_or_default()` / `.unwrap_or(value)` for non-critical fallbacks.

### Types and Structs

- Data models: `#[derive(Debug, Serialize)]`
- Askama templates: `#[derive(Template)]` with `#[template(path = "...")]` — always private, defined next to the handler
- Query params: `#[derive(Deserialize)]`
- CLI: `#[derive(Parser)]`, `#[derive(Subcommand)]`
- Methods on result structs for display logic (e.g., `human_size()`, `mime_category()`)
- No custom trait impls beyond derives. No newtype wrappers.

### Function Signatures

- Database functions take `&Connection` (never owned). Use `&mut Connection` only for transactions.
- Axum handlers: `pub async fn name(State(state): State<AppState>, ...) -> impl IntoResponse`
- Use `Option<&str>` for optional string filters, `&[i64]` for slices.
- Avoid explicit lifetime annotations unless required (only one instance in the codebase).

### Async Patterns

- Tokio runtime is created manually in `serve()` via `Runtime::new()` + `block_on`. `main()` is synchronous.
- Axum handlers are `pub async fn`. Database access uses `std::sync::Mutex` (not `tokio::Mutex`).
- Scope mutex locks to drop before `.await`: `let data = { let conn = state.db.lock().unwrap(); query(&conn) };`
- Use `tokio::task::spawn_blocking` for CPU-intensive work (image decoding).
- Use `tokio::process::Command` for subprocess calls (ffmpeg).

### Database Patterns (rusqlite)

- Single `Connection` in `Arc<Mutex<Connection>>` — no connection pool.
- Schema DDL as `pub const` in `db/schema.rs`.
- WAL mode, `PRAGMA foreign_keys = ON`, `PRAGMA cache_size = -64000`.
- Query patterns: `conn.query_row(...)` for single rows, `stmt.query_map(...).collect::<Result<Vec<_>, _>>()` for multi-row.
- Dynamic SQL via `format!()` for WHERE clauses; parameterized values via `?1`, `?2` positional params.
- Dynamic params: `Vec<Box<dyn ToSql>>` converted to `Vec<&dyn ToSql>`.
- Transactions for batch inserts (`prepare_cached`, 5000-row batches, explicit `tx.commit()`).
- Use `rusqlite::OptionalExtension` for queries that may return zero rows.

### Templates (Askama + HTMX)

- `base.html` with `{% block title %}` / `{% block content %}`.
- Same handler renders full page or HTMX partial based on `hx-request` header.
- Template structs are private with private fields.
- Always render via `Html(t.render().unwrap_or_default())`.

### Formatting

- Standard `rustfmt` defaults (4-space indent, K&R braces).
- Trailing commas in multi-line struct literals, function args, and enum variants.
- Method chains: one method per line with leading `.`
- Numeric literals use underscores: `1_073_741_824`, `978_307_200`.

### Testing

- Tests live in `#[cfg(test)] mod tests` at the bottom of source files.
- Test names: `test_<function>_<case>` — e.g., `test_normalize_phone_formatted`.
- All tests are synchronous. Use `tempfile::tempdir()` for ephemeral databases.
- Assertions with context messages: `assert!(cond, "explanation, got: {val}");`
- No integration test directory (`tests/`). Examples in `examples/` are manual scripts.

## Key Rules

- Do not write markdown files unless explicitly requested.
- Do not write comments unless they add real value. No restating-the-obvious comments.
- Prefer editing existing files over creating new ones.
