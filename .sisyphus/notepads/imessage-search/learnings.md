# Task 1 Learnings

## Date: 2026-03-01

### What was created
- Cargo.toml with all dependencies
- src/main.rs - clap CLI with import/serve subcommands
- src/state.rs - AppState stub
- src/error.rs - custom Error stub
- src/import/{mod,messages,contacts,attachments}.rs
- src/db/{mod,schema,queries}.rs
- src/search/mod.rs
- src/web/{mod,pages,partials,attachments}.rs
- src/models/mod.rs
- templates/base.html - askama layout skeleton
- static/js/htmx.min.js - v2.0.4 (50917 bytes)
- static/css/style.css - empty
- .gitignore - /target, data/, *.db files, .env

### Adjustments from plan
- Plan specified `askama = "0.13"` and `askama_web = "0.2"` but these versions don't exist on crates.io
- Used `askama = "0.15"` and `askama_web = "0.15"` instead (current latest compatible pair)
- cargo init defaulted to edition 2024; changed to 2021 as more stable

### Conventions established
- Module stubs use `pub fn placeholder() {}` pattern
- Dead code warnings are expected and acceptable for stubs

## Task 2 Learnings
- Added poc subcommand to copy chat.db and stream 100 messages with decoded text
- Used imessage_database tables::messages::Message stream_rows with QueryContext::default and generate_text
- Decode success rate for 100 messages: 93% (93 non-empty)

## Task 3 Learnings
- Implemented 5 tables + FTS5 virtual table + 6 indexes in src/db/schema.rs
- Models in src/models/mod.rs derive Debug + serde::Serialize
- create_db() sets WAL, synchronous=NORMAL, foreign_keys=ON, cache_size=-64000
- FTS5 uses trigram tokenizer (no unicode61/BM25 second table, no sync triggers)
- Had to fix pre-existing broken web module: askama_web WebTemplate derive wasn't generating IntoResponse impl
- Fixed by switching to manual Html(t.render().unwrap_or_default()) pattern
- Created stub templates (search, conversation, attachments, analytics, partials/) to unblock compilation
- Added tempfile as dev-dependency for tests
- All 5 schema tests pass: tables, FTS trigram, indexes, drop_and_recreate, pragmas

## Task 8 Learnings
- Implemented Axum web server with full routing, static file serving, and base template
- AppState holds Arc<Mutex<Connection>> and attachment_root: PathBuf
- Used #[derive(Template, WebTemplate)] from askama + askama_web for auto IntoResponse
- Router: 11 routes + /static/* via tower_http::services::ServeDir
- Downloaded Pico CSS v2 (83KB) to static/css/pico.min.css for local styling
- base.html has nav bar with links to all sections, includes pico.min.css + style.css + htmx.min.js
- Serve command uses tokio::runtime::Runtime::new() + block_on (non-async main)
- tracing_subscriber with env-filter for logging
- Askama templates: cannot use `ref` keyword in {% if let %} patterns
- All 9 routes + 3 static files return HTTP 200

## Task 9 Learnings
- Wired CLI import to call import::run_import and removed poc subcommand
- Added import::run_import to copy chat.db, recreate port DB, resolve contacts, and call messages import
