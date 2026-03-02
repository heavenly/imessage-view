# iMessage Search — Local Web App

## TL;DR

> **Quick Summary**: Build a Rust local web application that imports iMessage data from Apple's `chat.db` into an optimized SQLite+FTS5 database, providing fast text search, attachment discovery, and analytics through an Axum+htmx+Askama browser UI.
> 
> **Deliverables**:
> - CLI tool: `cargo run -- import` — imports chat.db into optimized local SQLite DB with FTS5 indexing
> - CLI tool: `cargo run -- serve` — starts local web server at `http://localhost:3000`
> - Web UI: Conversation list, message viewer, full-text search, attachment browser, basic analytics
> - Sub-100ms search across all messages via FTS5 trigram tokenizer
> 
> **Estimated Effort**: Large
> **Parallel Execution**: YES — 4 waves
> **Critical Path**: Task 1 → Task 2 → Task 3 → Task 5 → Task 8 → Task 10 → Task 13 → F1-F4

---

## Context

### Original Request
Build an iMessage chat.db port for fast, efficient searching through messages, files, attachments, and chats — solving the problem that macOS iMessage's built-in search is very poor.

### Interview Summary
**Key Discussions**:
- **Language**: Rust — best typedstream decoding via `imessage-database` crate, Tantivy available if needed, `rusqlite` for DB
- **Interface**: Local web UI — Axum + htmx + Askama (server-rendered, no JS build step)
- **Primary use cases** (priority order): Fast text search > Attachment discovery > Analytics/stats
- **Sync strategy**: One-time import (run once to import, re-run to refresh — drop and reimport)
- **Database**: SQLite + FTS5 with trigram tokenizer (single FTS table for v1, no BM25/unicode61 dual-table)
- **Tests**: No automated tests — QA scenarios only
- **UI framework**: Axum + htmx + Askama, following kanidm patterns (HxRequest for full/partial, block fragment rendering)

**Research Findings**:
- `imessage-database` crate v3.3.2 from ReagentX/imessage-exporter is the gold standard for reading chat.db in Rust
- `crabstep` crate handles typedstream deserialization (Apple's proprietary binary format for `attributedBody`)
- 99.6% of messages on modern macOS have NULL `text` column — must use `generate_text()` to decode `attributedBody`
- SQLite FTS5 trigram tokenizer provides substring matching but BM25 ranking is poor with trigrams; sufficient for v1
- FTS5 trigram requires 3+ char queries — need LIKE fallback for shorter queries
- External content table pattern with `INSERT INTO fts(fts) VALUES('rebuild')` after bulk import (no triggers needed)
- `rusqlite = "=0.38.0"` must be pinned to match `imessage-database`'s dependency
- Contact names are NOT in chat.db — stored in separate AddressBook SQLite DB (best-effort resolution)
- Attachment paths use `~` prefix in DB, need expansion to absolute paths
- Reactions are separate message rows (`associated_message_type != 0`) — filter during import
- Apple date format: nanoseconds since 2001-01-01 UTC
- `askama_web` provides `#[derive(WebTemplate)]` for auto IntoResponse
- `axum-htmx` provides `HxRequest` extractor for partial vs full page rendering
- `tower-http` fs feature for static file serving, `axum-extra` FileStream for large attachment downloads
- Chat deduplication needed: same contact can have separate SMS/iMessage/RCS chat rows

### Metis Review
**Identified Gaps** (all addressed in plan):
- Task 0 proof-of-concept needed before full build (verify `imessage-database` reads Tahoe chat.db)
- `rusqlite` version pinning critical (`=0.38.0` to match `imessage-database`)
- Short query (<3 char) fallback to LIKE needed
- AddressBook resolution should be best-effort, not hard requirement
- Must handle `\u{FFFC}` (attachment placeholder) and `\u{FFFD}` (app placeholder) in decoded text
- Chat deduplication via `person_centric_id` and `chat_lookup` table
- Need `HxRequestGuardLayer` on partial-only routes
- Vendor htmx.min.js locally (no CDN for local-only app)
- Use `indicatif` for import progress bars (CLI, not web UI for import)
- `FileStream` for attachment serving (files can be large videos)

---

## Work Objectives

### Core Objective
Import all iMessage data from Apple's `chat.db` into a search-optimized SQLite database and serve a fast, local web UI for searching messages, browsing conversations, discovering attachments, and viewing basic analytics.

### Concrete Deliverables
- `Cargo.toml` with all dependencies (single crate, not workspace)
- `src/` — Rust source code (import pipeline, search engine, web server)
- `templates/` — Askama HTML templates (base layout, conversation list, message viewer, search, attachments, analytics)
- `static/` — Vendored htmx.min.js + CSS
- Local SQLite database at `data/imessage.db` (created by import command)
- CLI with two subcommands: `import` and `serve`

### Definition of Done
- [ ] `cargo run -- import` completes without errors, imports all messages with decoded text
- [ ] `cargo run -- serve` starts server, `curl http://localhost:3000/` returns 200
- [ ] Search for a known message term returns results in <200ms
- [ ] Attachment browser shows files grouped by type
- [ ] Conversations display with contact names where available

### Must Have
- Full import of messages, conversations, contacts, and attachment metadata from chat.db
- `attributedBody` typedstream decoding via `imessage-database` crate (not just `text` column)
- FTS5 trigram search with LIKE fallback for <3 char queries
- Conversation list with last message preview, sorted by recency
- Message viewer with pagination (50 messages per page, infinite scroll via htmx `revealed` trigger)
- Search page with results highlighting and conversation context
- Attachment browser with MIME type filtering
- Basic analytics (message counts per conversation, messages over time)
- Contact name resolution from AddressBook (best-effort — always show phone/email, enhance with name)
- Attachment file serving through web server
- Progress bar during import (indicatif)

### Must NOT Have (Guardrails)
- **No Tantivy** — FTS5 trigram is sufficient for v1
- **No unicode61/BM25 second FTS table** — single trigram FTS table only
- **No workspace** — single crate with modules
- **No incremental sync** — drop-and-reimport only
- **No import web UI** — import runs in terminal with indicatif progress bar
- **No FTS sync triggers** — use `INSERT INTO fts(fts) VALUES('rebuild')` after bulk import
- **No CDN dependencies** — vendor htmx.min.js locally
- **No React/Svelte/JS build step** — pure Axum + htmx + Askama
- **No DuckDB or vector DB** — SQLite only for v1
- **No real-time message sync or file watchers**
- **No authentication or multi-user support** — localhost only
- **No over-abstraction** — favor straightforward code over premature generics/traits
- **No excessive comments** — let code speak, comment only non-obvious decisions

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed. No exceptions.

### Test Decision
- **Infrastructure exists**: NO (new project)
- **Automated tests**: NONE — QA scenarios only
- **Framework**: N/A

### QA Policy
Every task MUST include agent-executed QA scenarios.
Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **CLI/Import**: Use Bash — run import command, query SQLite, assert row counts and data integrity
- **Web UI**: Use Playwright (playwright skill) — navigate pages, interact with search, assert DOM content, screenshot
- **API/Routes**: Use Bash (curl) — send requests, assert status codes and response content
- **Search**: Use Bash (curl + sqlite3) — verify search results match expected, measure latency

---

## Execution Strategy

### Parallel Execution Waves

> **ORCHESTRATOR INSTRUCTION**: Within each wave, dispatch ALL listed tasks as parallel subagents simultaneously.
> Tasks within a wave have NO dependencies on each other. Wait for ALL tasks in a wave to complete before starting the next wave.
> The dependency matrix below is the authoritative source of truth for task ordering.

```
Wave 1 (Sequential bootstrap — must complete in order):
├── Task 1: Project scaffolding + Cargo.toml + module stubs [quick]
└── Task 2: Proof-of-concept — verify imessage-database reads chat.db [deep]
    NOTE: T1 → T2 are sequential (T2 needs T1's compiled project)

Wave 2 (After T2 completes — 3 PARALLEL subagents):
├── Task 3: Database schema + creation logic [quick]
├── Task 4: Contact resolution from AddressBook [unspecified-high]
└── Task 8: Web server shell (Axum + static files + routing) [unspecified-high]
    NOTE: T3, T4, T8 all depend ONLY on T2.
    T8 doesn't need imported data — it only needs the project to compile.
    Launch all 3 simultaneously.

Wave 3 (After T3+T4 complete — 1 subagent):
└── Task 5: Full message import pipeline [deep] (depends: T3 schema + T4 contacts)
    NOTE: T5 is the big import task. It needs the schema (T3) and contacts (T4).
    T8 may still be running from Wave 2 — that's fine, T5 doesn't depend on it.

Wave 4 (After T5 completes — 3 PARALLEL subagents):
├── Task 6: Attachment metadata import [unspecified-high] (depends: T3 + T5)
├── Task 7: FTS5 search setup + query logic [deep] (depends: T5)
└── Task 9: Analytics query functions [quick] (depends: T5)
    NOTE: T6 needs schema (T3) + messages imported (T5) for foreign keys.
    T7 needs messages in DB for FTS5 rebuild. T9 needs data to query.
    All 3 are independent of each other — launch simultaneously.

Wave 5 (After T6+T7+T8+T9 all complete — 5 PARALLEL subagents):
├── Task 10: Conversation list page [unspecified-high] (depends: T5 + T8)
├── Task 11: Message viewer with pagination [unspecified-high] (depends: T5 + T8)
├── Task 12: Search page with FTS5 results [unspecified-high] (depends: T7 + T8)
├── Task 13: Attachment browser + download [unspecified-high] (depends: T6 + T8)
└── Task 14: Analytics dashboard [quick] (depends: T8 + T9)
    NOTE: All 5 UI pages can run simultaneously.
    Each needs the web shell (T8) + their specific data dependency.
    T8 should be done by Wave 2. T6/T7/T9 done by Wave 4.
    Maximum parallel throughput here — 5 subagents!

Wave FINAL (After ALL implementation tasks — 4 PARALLEL review agents):
├── Task F1: Plan compliance audit [oracle]
├── Task F2: Code quality review [unspecified-high]
├── Task F3: Real manual QA [unspecified-high + playwright]
└── Task F4: Scope fidelity check [deep]
    NOTE: ALL 4 run simultaneously. All must APPROVE. Rejection → fix → re-run.

Critical Path: T1 → T2 → T3 → T5 → T7 → T12 → F1-F4
Parallel Speedup: ~65% faster than sequential (6 waves vs 14 sequential tasks)
Max Concurrent: 5 (Wave 5)
Total Subagent Dispatches: 18 (14 tasks + 4 reviews)
```

### Dependency Matrix

| Task | Depends On | Blocks | Wave | Run Parallel With |
|------|-----------|--------|------|-------------------|
| 1 | — | 2 | 1 | (sequential with T2) |
| 2 | 1 | 3, 4, 8 | 1 | (sequential with T1) |
| 3 | 2 | 5, 6, 7 | 2 | T4, T8 |
| 4 | 2 | 5 | 2 | T3, T8 |
| 8 | 2 | 10, 11, 12, 13, 14 | 2 | T3, T4 |
| 5 | 3, 4 | 6, 7, 9, 10, 11 | 3 | (solo — big import) |
| 6 | 3, 5 | 13 | 4 | T7, T9 |
| 7 | 5 | 12 | 4 | T6, T9 |
| 9 | 5 | 14 | 4 | T6, T7 |
| 10 | 5, 8 | — | 5 | T11, T12, T13, T14 |
| 11 | 5, 8 | — | 5 | T10, T12, T13, T14 |
| 12 | 7, 8 | — | 5 | T10, T11, T13, T14 |
| 13 | 6, 8 | — | 5 | T10, T11, T12, T14 |
| 14 | 8, 9 | — | 5 | T10, T11, T12, T13 |
| F1-F4 | ALL | — | FINAL | F1, F2, F3, F4 |

### Agent Dispatch Summary

- **Wave 1**: 2 tasks (sequential) — T1 → `quick`, T2 → `deep`
- **Wave 2**: **3 parallel subagents** — T3 → `quick`, T4 → `unspecified-high`, T8 → `unspecified-high`
- **Wave 3**: **1 subagent** — T5 → `deep` (the big import task)
- **Wave 4**: **3 parallel subagents** — T6 → `unspecified-high`, T7 → `deep`, T9 → `quick`
- **Wave 5**: **5 parallel subagents** — T10-T13 → `unspecified-high`, T14 → `quick`
- **FINAL**: **4 parallel subagents** — F1 → `oracle`, F2 → `unspecified-high`, F3 → `unspecified-high` + `playwright`, F4 → `deep`

## TODOs

- [x] 1. Project Scaffolding + Cargo.toml + Module Stubs

  **What to do**:
  - Run `cargo init` in the project directory
  - Create `Cargo.toml` with these exact dependencies:
    ```toml
    [package]
    name = "imessage-search"
    version = "0.1.0"
    edition = "2021"

    [dependencies]
    imessage-database = "3.3.2"
    rusqlite = { version = "=0.38.0", features = ["bundled"] }
    axum = "0.8"
    axum-extra = { version = "0.10", features = ["attachment", "file-stream"] }
    axum-htmx = "0.6"
    askama = "0.13"
    askama_web = "0.2"
    tokio = { version = "1", features = ["full"] }
    tower-http = { version = "0.6", features = ["fs", "trace"] }
    tower = "0.5"
    serde = { version = "1", features = ["derive"] }
    chrono = "0.4"
    indicatif = "0.17"
    tracing = "0.1"
    tracing-subscriber = { version = "0.3", features = ["env-filter"] }
    clap = { version = "4", features = ["derive"] }
    anyhow = "1"
    ```
  - Create directory structure:
    ```
    src/
      main.rs              # CLI entry point with clap (import/serve subcommands)
      state.rs             # AppState struct (DB pool, config paths)
      error.rs             # Custom error type wrapping anyhow
      import/
        mod.rs             # Import orchestration
        messages.rs        # Message import + text decode (stub)
        contacts.rs        # AddressBook resolution (stub)
        attachments.rs     # Attachment metadata import (stub)
      db/
        mod.rs             # DB connection + init
        schema.rs          # CREATE TABLE statements (stub)
        queries.rs         # Query functions (stub)
      search/
        mod.rs             # FTS5 setup + search handlers (stub)
      web/
        mod.rs             # Router assembly (stub)
        pages.rs           # Full-page route handlers (stub)
        partials.rs        # htmx partial route handlers (stub)
        attachments.rs     # File download handlers (stub)
      models/
        mod.rs             # Domain types (stub)
    templates/
      base.html            # Layout skeleton with htmx script tag
    static/
      js/htmx.min.js       # Vendored htmx (download from unpkg.com/htmx.org@2.0.4/dist/htmx.min.js)
      css/style.css         # Empty CSS file
    ```
  - `main.rs` should parse CLI args with clap (`import` and `serve` subcommands), print "not implemented" for each
  - All module stubs should compile (`cargo check` must pass)
  - Download htmx.min.js v2.0.4 from unpkg CDN and save to `static/js/htmx.min.js`

  **Must NOT do**:
  - Do NOT create a Cargo workspace — single crate only
  - Do NOT add Tantivy, DuckDB, or any other search/DB dependency
  - Do NOT write implementation logic — stubs only (empty functions with `todo!()` or `unimplemented!()`)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 1 (sequential — must complete before Task 2)
  - **Blocks**: Tasks 2-14 (everything depends on project existing)
  - **Blocked By**: None

  **References**:

  **Pattern References**:
  - None — greenfield project

  **API/Type References**:
  - `imessage-database` crate: https://docs.rs/imessage-database/3.3.2/
  - `axum` crate: https://docs.rs/axum/0.8/
  - `clap` derive: https://docs.rs/clap/4/clap/_derive/index.html
  - `askama_web`: https://docs.rs/askama_web/

  **External References**:
  - htmx download: `https://unpkg.com/htmx.org@2.0.4/dist/htmx.min.js`

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: Project compiles successfully
    Tool: Bash
    Preconditions: Cargo.toml and all stubs created
    Steps:
      1. Run `cargo check` in project root
      2. Check exit code is 0
    Expected Result: Clean compilation with no errors
    Failure Indicators: Any `error[E...]` in output
    Evidence: .sisyphus/evidence/task-1-cargo-check.txt

  Scenario: CLI shows help text
    Tool: Bash
    Preconditions: Project compiles
    Steps:
      1. Run `cargo run -- --help`
      2. Check output contains "import" and "serve"
    Expected Result: Help text lists both subcommands
    Failure Indicators: Missing subcommand names or panic
    Evidence: .sisyphus/evidence/task-1-cli-help.txt

  Scenario: htmx.min.js is vendored
    Tool: Bash
    Preconditions: Download completed
    Steps:
      1. Check file exists: `test -f static/js/htmx.min.js`
      2. Check file size > 10KB: `test $(wc -c < static/js/htmx.min.js) -gt 10000`
    Expected Result: htmx.min.js exists and is >10KB
    Failure Indicators: File missing or empty
    Evidence: .sisyphus/evidence/task-1-htmx-vendored.txt
  ```

  **Commit**: YES
  - Message: `feat: scaffold project with Cargo.toml and module stubs`
  - Files: `Cargo.toml, src/**, templates/**, static/**`

- [x] 2. Proof-of-Concept — Verify imessage-database Reads chat.db

  **What to do**:
  - In `src/main.rs`, implement a temporary `poc` function that:
    1. Copies `~/Library/Messages/chat.db` to `data/source_chat.db` (never read the original in-place — copy first for safety)
    2. Opens the copy with `imessage-database` crate
    3. Iterates 100 messages using the crate's `Message::stream()` API
    4. Calls `message.generate_text(&db)` on each to decode `attributedBody`
    5. Prints: message ROWID, decoded text (first 80 chars), date, is_from_me
    6. Reports: how many had NULL text before decode vs after, success/failure counts
  - Wire the `poc` function to a temporary `poc` CLI subcommand
  - Create `data/` directory (add to .gitignore)
  - This is a GATE: if `generate_text()` fails on most messages, we need to investigate before proceeding

  **Must NOT do**:
  - Do NOT modify `~/Library/Messages/chat.db` — read-only copy
  - Do NOT build the full import pipeline — just verify the crate works
  - Do NOT create the port database schema yet

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (sequential with Task 1)
  - **Parallel Group**: Wave 1
  - **Blocks**: Tasks 3-14 (GATE — must pass before continuing)
  - **Blocked By**: Task 1

  **References**:

  **Pattern References**:
  - `imessage-database` Message model: https://docs.rs/imessage-database/3.3.2/imessage_database/tables/messages/message/struct.Message.html
  - `Table::stream()` API for iterating rows
  - `Message::generate_text(&db)` for typedstream decoding

  **API/Type References**:
  - `imessage-database::tables::table::Table` trait — `stream()` method returns all rows
  - `Message` struct fields: `rowid`, `guid`, `text`, `date`, `is_from_me`, `associated_message_type`
  - Date conversion: `date / 1_000_000_000 + 978307200` → Unix timestamp

  **External References**:
  - imessage-exporter source (how they use the crate): https://github.com/ReagentX/imessage-exporter/blob/develop/imessage-exporter/src/app/
  - Apple typedstream format context: `attributedBody` column stores `NSMutableAttributedString` in typedstream format. `generate_text()` handles decoding via `crabstep` crate internally.

  **WHY Each Reference Matters**:
  - The `Table::stream()` + `generate_text()` pattern is how imessage-exporter itself reads messages — follow their exact API usage
  - Date conversion formula is Apple-specific (nanoseconds since 2001-01-01)
  - Copying chat.db first is critical — the original may have proprietary SQLite triggers (macOS Sequoia) that break standard rusqlite

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: POC reads 100 messages from chat.db copy
    Tool: Bash
    Preconditions: ~/Library/Messages/chat.db exists, Full Disk Access granted
    Steps:
      1. Run `cargo run -- poc 2>&1 | tee /tmp/poc-output.txt`
      2. Count lines containing decoded text: `grep -c 'text:' /tmp/poc-output.txt`
      3. Check for error count line in output
    Expected Result: At least 80 of 100 messages have non-empty decoded text
    Failure Indicators: Panic, 0 messages read, or >50% decode failures
    Evidence: .sisyphus/evidence/task-2-poc-output.txt

  Scenario: chat.db is copied, not read in-place
    Tool: Bash
    Preconditions: POC has run
    Steps:
      1. Check `data/source_chat.db` exists: `test -f data/source_chat.db`
      2. Verify it's a valid SQLite file: `sqlite3 data/source_chat.db "SELECT COUNT(*) FROM message;"`
    Expected Result: Copy exists and is queryable
    Failure Indicators: File missing or corrupt
    Evidence: .sisyphus/evidence/task-2-db-copy.txt
  ```

  **Commit**: YES
  - Message: `feat: verify imessage-database reads chat.db (proof of concept)`
  - Files: `src/main.rs, .gitignore`

- [x] 3. Database Schema + Creation Logic

  **What to do**:
  - Implement `src/db/schema.rs` with all CREATE TABLE statements:
    ```sql
    CREATE TABLE IF NOT EXISTS contacts (
        id INTEGER PRIMARY KEY,
        handle TEXT NOT NULL UNIQUE,
        display_name TEXT,
        service TEXT,
        person_centric_id TEXT
    );

    CREATE TABLE IF NOT EXISTS conversations (
        id INTEGER PRIMARY KEY,
        apple_chat_id INTEGER,
        guid TEXT UNIQUE,
        display_name TEXT,
        is_group BOOLEAN NOT NULL,
        service TEXT,
        last_message_date INTEGER,
        message_count INTEGER DEFAULT 0,
        participant_count INTEGER DEFAULT 0
    );

    CREATE TABLE IF NOT EXISTS conversation_participants (
        conversation_id INTEGER REFERENCES conversations(id),
        contact_id INTEGER REFERENCES contacts(id),
        PRIMARY KEY (conversation_id, contact_id)
    );

    CREATE TABLE IF NOT EXISTS messages (
        id INTEGER PRIMARY KEY,
        apple_message_id INTEGER UNIQUE,
        guid TEXT UNIQUE,
        conversation_id INTEGER NOT NULL REFERENCES conversations(id),
        sender_id INTEGER REFERENCES contacts(id),
        is_from_me BOOLEAN NOT NULL,
        body TEXT,
        date_unix INTEGER NOT NULL,
        service TEXT,
        is_reaction BOOLEAN DEFAULT FALSE,
        reaction_type INTEGER,
        thread_originator_guid TEXT,
        is_edited BOOLEAN DEFAULT FALSE,
        has_attachments BOOLEAN DEFAULT FALSE,
        balloon_bundle_id TEXT
    );

    CREATE TABLE IF NOT EXISTS attachments (
        id INTEGER PRIMARY KEY,
        message_id INTEGER NOT NULL REFERENCES messages(id),
        apple_attachment_id INTEGER,
        guid TEXT,
        filename TEXT,
        resolved_path TEXT,
        mime_type TEXT,
        uti TEXT,
        transfer_name TEXT,
        total_bytes INTEGER,
        file_exists BOOLEAN DEFAULT FALSE
    );

    CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
        body, content='messages', content_rowid='id', tokenize='trigram'
    );

    -- Performance indexes
    CREATE INDEX IF NOT EXISTS idx_messages_conversation_date ON messages(conversation_id, date_unix DESC);
    CREATE INDEX IF NOT EXISTS idx_messages_sender ON messages(sender_id);
    CREATE INDEX IF NOT EXISTS idx_messages_date ON messages(date_unix DESC);
    CREATE INDEX IF NOT EXISTS idx_attachments_message ON attachments(message_id);
    CREATE INDEX IF NOT EXISTS idx_attachments_mime ON attachments(mime_type);
    CREATE INDEX IF NOT EXISTS idx_contacts_handle ON contacts(handle);
    ```
  - Implement `src/db/mod.rs`:
    - `create_db(path: &str) -> Result<Connection>` — creates/opens SQLite DB, runs all CREATE statements
    - `drop_and_recreate(path: &str) -> Result<Connection>` — drops all tables first (for reimport), then creates
    - Set pragmas: `journal_mode=WAL`, `synchronous=NORMAL`, `foreign_keys=ON`, `cache_size=-64000` (64MB)
  - Implement `src/models/mod.rs` with Rust structs matching each table (derive `Debug`, `serde::Serialize`)

  **Must NOT do**:
  - Do NOT add a unicode61/BM25 second FTS table
  - Do NOT add FTS sync triggers — we use `'rebuild'` after bulk import
  - Do NOT store `date_iso` or `raw_attributed_body` — keep schema lean

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 4 and 8)
  - **Parallel Group**: Wave 2
  - **Blocks**: Tasks 5, 6, 7
  - **Blocked By**: Task 2

  **References**:

  **Pattern References**:
  - Schema design from interview phase (see Context section above)
  - `rusqlite` connection setup: https://docs.rs/rusqlite/0.38.0/rusqlite/

  **API/Type References**:
  - `rusqlite::Connection::open()` for DB creation
  - `connection.execute_batch()` for running multiple DDL statements
  - FTS5 external content table docs: https://www.sqlite.org/fts5.html#external_content_tables

  **WHY Each Reference Matters**:
  - The FTS5 `content='messages'` pattern means the FTS table doesn't store its own copy of text — it references the `messages` table, saving ~50% disk space
  - WAL journal mode is critical for concurrent reads during web serving while import may be running
  - `cache_size=-64000` uses 64MB of cache, dramatically speeding up bulk inserts

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: Database creates successfully with all tables
    Tool: Bash
    Preconditions: Task 2 completed, cargo compiles
    Steps:
      1. Run a minimal test: `cargo run -- import --dry-run` (or add temp code to create DB only)
      2. Query tables: `sqlite3 data/imessage.db ".tables"`
      3. Verify all 5 tables + 1 FTS table exist
    Expected Result: Output includes: contacts, conversations, conversation_participants, messages, attachments, messages_fts
    Failure Indicators: Missing tables or SQLite errors
    Evidence: .sisyphus/evidence/task-3-schema.txt

  Scenario: FTS5 virtual table is trigram tokenizer
    Tool: Bash
    Preconditions: Database created
    Steps:
      1. Run: `sqlite3 data/imessage.db "SELECT * FROM sqlite_master WHERE type='table' AND name='messages_fts'"`
      2. Check output contains "tokenize='trigram'"
    Expected Result: FTS5 table uses trigram tokenizer
    Failure Indicators: Wrong tokenizer or table missing
    Evidence: .sisyphus/evidence/task-3-fts-tokenizer.txt
  ```

  **Commit**: YES
  - Message: `feat: add database schema creation and migration`
  - Files: `src/db/mod.rs, src/db/schema.rs, src/models/mod.rs`

- [x] 4. Contact Resolution from AddressBook

  **What to do**:
  - Implement `src/import/contacts.rs`:
    - Find AddressBook database(s) at `~/Library/Application Support/AddressBook/Sources/*/AddressBook-v22.abcddb`
    - Also check `~/Library/Application Support/AddressBook/AddressBook-v22.abcddb` (main DB)
    - Open each as read-only SQLite, query for contacts with phone numbers and emails
    - Build a `HashMap<String, String>` mapping normalized phone/email → display name
    - Phone normalization: strip all non-digit chars, handle +1 prefix for US numbers
    - Email normalization: lowercase
    - Return the map for use during message import
  - This is **best-effort**: if AddressBook is not accessible or empty, return empty map and continue
  - Log warnings (not errors) if AddressBook can't be read
  - The AddressBook schema has tables `ZABCDRECORD` (contacts) with `ZFIRSTNAME`, `ZLASTNAME`, and `ZABCDEMAILADDRESS` / `ZABCDPHONENUMBER` linked via `ZOWNER`

  **Must NOT do**:
  - Do NOT fail the import if AddressBook is not accessible
  - Do NOT try to resolve contacts from any cloud/API source
  - Do NOT cache the AddressBook data to disk — rebuild each import

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 3)
  - **Parallel Group**: Wave 2
  - **Blocks**: Task 5 (provides contact map for message import)
  - **Blocked By**: Task 2

  **References**:

  **Pattern References**:
  - AddressBook DB location: `~/Library/Application Support/AddressBook/Sources/*/AddressBook-v22.abcddb`
  - Schema tables: `ZABCDRECORD` (ZFIRSTNAME, ZLASTNAME), `ZABCDEMAILADDRESS` (ZADDRESSNORMALIZED, ZOWNER), `ZABCDPHONENUMBER` (ZFULLNUMBER, ZOWNER)

  **External References**:
  - AddressBook schema is undocumented by Apple — the table/column names above come from direct inspection
  - `person_centric_id` field on the `handle` table in chat.db can also be used to merge handles for the same person

  **WHY Each Reference Matters**:
  - AddressBook databases may be in per-source subdirectories (iCloud, Exchange, local) — must glob for all of them
  - Phone normalization is critical because chat.db stores `+15551234567` but AddressBook might store `(555) 123-4567`
  - Best-effort is key because Full Disk Access may not cover AddressBook, or the user may have no local contacts

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: Contact resolution finds at least some names
    Tool: Bash
    Preconditions: AddressBook exists on this machine
    Steps:
      1. Run import with contact resolution enabled
      2. Query: `sqlite3 data/imessage.db "SELECT COUNT(*) FROM contacts WHERE display_name IS NOT NULL"`
    Expected Result: Count > 0 (at least some contacts resolved)
    Failure Indicators: All display_names are NULL
    Evidence: .sisyphus/evidence/task-4-contacts.txt

  Scenario: Import succeeds even if AddressBook is inaccessible
    Tool: Bash
    Preconditions: N/A
    Steps:
      1. Check that import doesn't panic or error if contact resolution returns empty map
      2. Verify contacts table has rows with NULL display_name (phone/email still stored)
    Expected Result: Import completes, contacts have handle but possibly NULL display_name
    Failure Indicators: Panic or error mentioning AddressBook
    Evidence: .sisyphus/evidence/task-4-contacts-fallback.txt
  ```

  **Commit**: YES
  - Message: `feat: add AddressBook contact resolution`
  - Files: `src/import/contacts.rs`

- [x] 5. Message Import Pipeline (Full Import with Text Decoding)

  **What to do**:
  - Implement `src/import/messages.rs` and `src/import/mod.rs`:
    1. Open the copied `data/source_chat.db` using `imessage-database` crate
    2. Open the port DB (`data/imessage.db`) using `rusqlite`
    3. Import flow:
       a. First pass: Import all handles from chat.db `handle` table → `contacts` table, using the contact map from Task 4 to set `display_name`
       b. Second pass: Import all chats from chat.db `chat` table → `conversations` table, linking participants via `chat_handle_join`
       c. Third pass: Stream all messages via `Message::stream()`, for each:
          - Skip reactions (`associated_message_type != 0` — types 1000-4000)
          - Call `message.generate_text(&db)` to decode `attributedBody` typedstream into plain text
          - Strip `\u{FFFC}` (attachment placeholders) and `\u{FFFD}` (app placeholders) from decoded text
          - Convert Apple nanosecond date to Unix timestamp: `date / 1_000_000_000 + 978307200`
          - Map `handle_id` → `sender_id` via the handles already imported
          - Map `chat_id` via `chat_message_join` → `conversation_id`
          - Handle chat deduplication: use `person_centric_id` on handles to merge duplicate contacts, and `chat_lookup` table if available (Sequoia+)
          - Insert into `messages` table
       d. After all messages inserted: `INSERT INTO messages_fts(messages_fts) VALUES('rebuild')` to populate FTS5 index
       e. Update `conversations` table: set `last_message_date`, `message_count`, `participant_count` from actual data
    4. Use `indicatif` progress bar showing: messages imported / total, with ETA
    5. Use `rusqlite` transactions — batch inserts in chunks of 5000 for performance
    6. Total import should work on the user's real chat.db (likely 100K-1M+ messages)
  - Wire to `import` CLI subcommand: copy chat.db → create/drop port DB → run import pipeline
  - Remove the `poc` subcommand (no longer needed)

  **Must NOT do**:
  - Do NOT modify the source chat.db — read-only
  - Do NOT create FTS sync triggers — only use `'rebuild'` command after bulk import
  - Do NOT import reactions as separate messages — skip them entirely for v1
  - Do NOT store `raw_attributed_body` BLOB in port DB
  - Do NOT implement incremental sync — always drop and reimport

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (solo — large import task)
  - **Parallel Group**: Wave 3 (after Tasks 3 and 4 complete)
  - **Blocks**: Tasks 6, 7, 9, 10, 11
  - **Blocked By**: Tasks 3, 4

  **References**:

  **Pattern References**:
  - `imessage-database` crate API: `Table::stream()` returns all rows for a table
  - `Message::generate_text(&db)` — the key method that decodes typedstream `attributedBody` into plain text. Uses `crabstep::TypedStreamDeserializer` internally with fallback to legacy byte-pattern parser
  - `Message` struct fields: `rowid`, `guid`, `text`, `date`, `is_from_me`, `handle_id`, `associated_message_type`, `thread_originator_guid`, `date_edited`, `balloon_bundle_id`
  - Date conversion: `TIMESTAMP_FACTOR = 1_000_000_000`, offset = `978307200` (seconds between Unix epoch and Apple epoch 2001-01-01)
  - Chat deduplication: `chat_handle_join` table maps chat_id → handle_id. `person_centric_id` on handle merges a contact's phone/email/services

  **API/Type References**:
  - `imessage_database::tables::table::Table` trait — `stream()` method
  - `imessage_database::tables::messages::message::Message` struct
  - `imessage_database::tables::chat::Chat` struct
  - `imessage_database::tables::handle::Handle` struct
  - `rusqlite::Transaction` for batch inserts

  **External References**:
  - imessage-exporter source showing how they iterate messages: https://github.com/ReagentX/imessage-exporter/blob/develop/imessage-exporter/src/app/
  - FTS5 rebuild command: https://www.sqlite.org/fts5.html#the_rebuild_command

  **WHY Each Reference Matters**:
  - `generate_text()` is the ONLY reliable way to get message text on Ventura+ (99.6% of messages have NULL `text` column)
  - Reactions must be filtered by `associated_message_type` — they're stored as regular message rows and would pollute search results
  - Batch inserts in transactions are 100x faster than individual inserts for SQLite
  - The FTS5 `'rebuild'` command is specifically designed for external content tables after bulk population

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: Full import completes on real chat.db
    Tool: Bash
    Preconditions: ~/Library/Messages/chat.db accessible, Full Disk Access granted
    Steps:
      1. Run `cargo run -- import 2>&1 | tee /tmp/import-output.txt`
      2. Check exit code is 0
      3. Query message count: `sqlite3 data/imessage.db "SELECT COUNT(*) FROM messages;"`
      4. Query decoded text: `sqlite3 data/imessage.db "SELECT COUNT(*) FROM messages WHERE body IS NOT NULL AND body != '';"`
    Expected Result: Message count > 0, at least 80% of messages have non-empty body text
    Failure Indicators: Panic, zero messages, or <50% text decode rate
    Evidence: .sisyphus/evidence/task-5-import-output.txt

  Scenario: FTS5 index populated after import
    Tool: Bash
    Preconditions: Import completed
    Steps:
      1. Run: `sqlite3 data/imessage.db "SELECT COUNT(*) FROM messages_fts;"`
      2. Run a search: `sqlite3 data/imessage.db "SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH 'hello';"`
    Expected Result: FTS row count matches messages with non-empty body, search returns > 0 results
    Failure Indicators: FTS table empty or search returns 0 for common words
    Evidence: .sisyphus/evidence/task-5-fts-populated.txt

  Scenario: Conversations have correct metadata
    Tool: Bash
    Preconditions: Import completed
    Steps:
      1. Run: `sqlite3 data/imessage.db "SELECT COUNT(*) FROM conversations WHERE message_count > 0;"`
      2. Run: `sqlite3 data/imessage.db "SELECT id, display_name, message_count, last_message_date FROM conversations ORDER BY message_count DESC LIMIT 5;"`
    Expected Result: Multiple conversations with non-zero message counts and valid dates
    Failure Indicators: All message_counts are 0 or NULL
    Evidence: .sisyphus/evidence/task-5-conversations.txt

  Scenario: Reactions are NOT imported as messages
    Tool: Bash
    Preconditions: Import completed
    Steps:
      1. Run: `sqlite3 data/imessage.db "SELECT COUNT(*) FROM messages WHERE is_reaction = 1;"`
    Expected Result: Count is 0 (reactions were skipped during import)
    Failure Indicators: Non-zero count
    Evidence: .sisyphus/evidence/task-5-no-reactions.txt
  ```

  **Commit**: YES
  - Message: `feat: implement full message import pipeline with typedstream decoding`
  - Files: `src/import/mod.rs, src/import/messages.rs, src/main.rs`

- [ ] 6. Attachment Metadata Import

  **What to do**:
  - Implement `src/import/attachments.rs`:
    1. After messages are imported (in the same import pipeline), iterate `attachment` table from chat.db
    2. For each attachment:
       - Read: `rowid`, `guid`, `filename`, `uti`, `mime_type`, `transfer_name`, `total_bytes`
       - Resolve the full file path: replace `~` prefix with user's home directory
       - Check if the resolved file actually exists on disk (`std::path::Path::exists()`)
       - Find the associated message via `message_attachment_join` table
       - Map the Apple message ROWID to our port DB's `messages.id`
       - Insert into `attachments` table with `file_exists` boolean
    3. Use batch inserts (same transaction chunking as messages)
    4. Log summary: total attachments, how many files exist on disk, common MIME types
  - If an attachment's message doesn't exist in our DB (because it was a reaction or orphaned), skip it
  - Handle NULL mime_type: fall back to UTI-based detection (e.g., `com.apple.coreaudio-format` → `audio/x-caf`)

  **Must NOT do**:
  - Do NOT copy attachment files — only import metadata and resolve paths
  - Do NOT fail if files are missing — just set `file_exists = FALSE`
  - Do NOT import sticker attachments as separate items (they're part of tapback messages which we skip)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 7 and 9)
  - **Parallel Group**: Wave 4
  - **Blocks**: Task 13
  - **Blocked By**: Tasks 3, 5 (needs port DB schema + messages imported)

  **References**:

  **Pattern References**:
  - `imessage-database` Attachment struct: fields include `rowid`, `filename`, `uti`, `mime_type`, `transfer_name`, `total_bytes`, `is_sticker`, `hide_attachment`
  - Attachment path resolution: `filename` column stores `~/Library/Messages/Attachments/xx/yy/guid/transfer_name` — replace `~` with home dir
  - MIME type fallback from UTI: `com.apple.coreaudio-format` → `audio/x-caf; codecs=opus`
  - `message_attachment_join` table: `message_id` → `attachment_id` mapping

  **External References**:
  - imessage-exporter attachment handling: https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/attachment.rs

  **WHY Each Reference Matters**:
  - Path resolution with `~` expansion is the #1 gotcha — the DB stores relative paths, you need absolute paths to serve files
  - `file_exists` check is critical for UX — users may have deleted files or not synced from iCloud
  - MIME type fallback prevents NULL mime_type entries that would break attachment filtering in the browser UI

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: Attachment metadata imported with file existence check
    Tool: Bash
    Preconditions: Messages imported (Task 5 complete)
    Steps:
      1. Run import (attachments are part of the import pipeline)
      2. Query: `sqlite3 data/imessage.db "SELECT COUNT(*) FROM attachments;"`
      3. Query: `sqlite3 data/imessage.db "SELECT COUNT(*) FROM attachments WHERE file_exists = 1;"`
      4. Query: `sqlite3 data/imessage.db "SELECT mime_type, COUNT(*) FROM attachments GROUP BY mime_type ORDER BY COUNT(*) DESC LIMIT 10;"`
    Expected Result: Total attachments > 0, some files exist, MIME type distribution looks reasonable
    Failure Indicators: Zero attachments or all file_exists = 0
    Evidence: .sisyphus/evidence/task-6-attachments.txt

  Scenario: Attachments link to valid messages
    Tool: Bash
    Preconditions: Import completed
    Steps:
      1. Run: `sqlite3 data/imessage.db "SELECT COUNT(*) FROM attachments WHERE message_id NOT IN (SELECT id FROM messages);"`
    Expected Result: Count is 0 (no orphaned attachments)
    Failure Indicators: Non-zero count (foreign key violation)
    Evidence: .sisyphus/evidence/task-6-attachment-integrity.txt
  ```

  **Commit**: YES
  - Message: `feat: import attachment metadata from chat.db`
  - Files: `src/import/attachments.rs, src/import/mod.rs`

- [ ] 7. FTS5 Search Setup + Query Logic

  **What to do**:
  - Implement `src/search/mod.rs`:
    1. `search(conn: &Connection, query: &str, limit: usize, offset: usize) -> Result<Vec<SearchResult>>`
       - If query length >= 3: use FTS5 trigram search
         ```sql
         SELECT m.id, m.body, m.date_unix, m.is_from_me, m.conversation_id,
                c.display_name as conversation_name,
                ct.display_name as sender_name, ct.handle as sender_handle,
                highlight(messages_fts, 0, '<mark>', '</mark>') as highlighted
         FROM messages_fts
         JOIN messages m ON m.id = messages_fts.rowid
         JOIN conversations c ON c.id = m.conversation_id
         LEFT JOIN contacts ct ON ct.id = m.sender_id
         WHERE messages_fts MATCH ?1
         ORDER BY m.date_unix DESC
         LIMIT ?2 OFFSET ?3
         ```
       - If query length < 3: fall back to LIKE search
         ```sql
         SELECT ... FROM messages m
         JOIN conversations c ON c.id = m.conversation_id
         LEFT JOIN contacts ct ON ct.id = m.sender_id
         WHERE m.body LIKE '%' || ?1 || '%'
         ORDER BY m.date_unix DESC
         LIMIT ?2 OFFSET ?3
         ```
    2. `SearchResult` struct: `id`, `body`, `highlighted_body`, `date_unix`, `is_from_me`, `conversation_id`, `conversation_name`, `sender_name`, `sender_handle`
    3. `search_count(conn: &Connection, query: &str) -> Result<usize>` — count total results for pagination
    4. Properly escape FTS5 special characters in the query (double-quote phrases, escape `*` `OR` `AND` `NOT` etc.)
  - The FTS5 trigram `MATCH` accepts plain text (no special query syntax needed for trigram tokenizer — it does substring matching)

  **Must NOT do**:
  - Do NOT add a second FTS table (unicode61/BM25)
  - Do NOT add Tantivy
  - Do NOT implement fuzzy/typo-tolerant search

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 6 and 9)
  - **Parallel Group**: Wave 4
  - **Blocks**: Task 12
  - **Blocked By**: Task 5 (needs messages + FTS index populated)

  **References**:

  **Pattern References**:
  - FTS5 trigram tokenizer: https://www.sqlite.org/fts5.html#the_experimental_trigram_tokenizer
  - FTS5 highlight function: `highlight(table, column_idx, open_marker, close_marker)`
  - FTS5 external content table queries join via `rowid`

  **External References**:
  - SQLite FTS5 full docs: https://www.sqlite.org/fts5.html
  - Trigram tokenizer specifics: queries under 3 chars return empty results — MUST use LIKE fallback

  **WHY Each Reference Matters**:
  - Trigram tokenizer doesn't need special query syntax — plain strings work for substring matching
  - The `highlight()` function is built into FTS5 — generates HTML-marked matches, no need to implement highlighting manually
  - LIKE fallback for <3 char queries is essential — users searching "hi" or "ok" should get results

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: FTS5 search returns results for common word
    Tool: Bash
    Preconditions: Import completed with FTS5 index
    Steps:
      1. Run: `sqlite3 data/imessage.db "SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH 'hello';"`
      2. Verify count > 0
    Expected Result: Multiple matches for a common word
    Failure Indicators: Zero results
    Evidence: .sisyphus/evidence/task-7-fts-search.txt

  Scenario: Short query (<3 chars) uses LIKE fallback
    Tool: Bash
    Preconditions: Import completed
    Steps:
      1. Call search function with query "hi" (2 chars)
      2. Verify it returns results (via LIKE fallback, not FTS5)
    Expected Result: Results found via LIKE '%hi%' query
    Failure Indicators: Empty results or FTS5 error
    Evidence: .sisyphus/evidence/task-7-short-query.txt

  Scenario: Search results include highlighting
    Tool: Bash
    Preconditions: Import completed
    Steps:
      1. Run search for "hello" via the search function
      2. Check highlighted_body contains '<mark>hello</mark>'
    Expected Result: HTML highlight markers wrap matched text
    Failure Indicators: No markers in highlighted text
    Evidence: .sisyphus/evidence/task-7-highlighting.txt
  ```

  **Commit**: YES
  - Message: `feat: add FTS5 trigram search with LIKE fallback`
  - Files: `src/search/mod.rs`

- [x] 8. Web Server Shell (Axum + Static Files + Base Template + Routing)

  **What to do**:
  - Implement `src/web/mod.rs` — Axum router assembly:
    ```rust
    pub fn create_router(state: AppState) -> Router {
        Router::new()
            .route("/", get(pages::index))
            .route("/conversations/{id}", get(pages::conversation))
            .route("/search", get(pages::search))
            .route("/attachments", get(pages::attachments_page))
            .route("/attachments/download/{id}", get(attachments::download))
            .route("/analytics", get(pages::analytics))
            // htmx partial routes
            .route("/partials/messages", get(partials::messages_partial))
            .route("/partials/conversations", get(partials::conversations_partial))
            .route("/partials/search-results", get(partials::search_results_partial))
            .nest_service("/static", ServeDir::new("static"))
            .with_state(state)
    }
    ```
  - Implement `src/state.rs` — `AppState` struct:
    ```rust
    #[derive(Clone)]
    pub struct AppState {
        pub db: Arc<Mutex<Connection>>,  // or use r2d2 connection pool
        pub attachment_root: PathBuf,     // ~/Library/Messages/Attachments/
    }
    ```
  - Implement `templates/base.html` — Askama base layout:
    - HTML5 doctype, responsive meta viewport
    - `<script src="/static/js/htmx.min.js"></script>` in head
    - `<link rel="stylesheet" href="/static/css/style.css">` in head
    - Navigation bar with links: Home, Search, Attachments, Analytics
    - `{% block content %}{% endblock %}` for page content
    - Use Pico CSS (download and vendor to `static/css/pico.min.css`) for clean default styling, or write minimal custom CSS
  - Wire `serve` CLI subcommand in `main.rs`:
    - Open port DB connection
    - Create AppState
    - Start Axum server on `0.0.0.0:3000`
    - Print URL to terminal: `Server running at http://localhost:3000`
  - Set up `tracing_subscriber` with env filter for request logging
  - Use `tower-http::trace::TraceLayer` for HTTP request/response logging

  **Must NOT do**:
  - Do NOT implement page handler logic yet — just return placeholder HTML from each route
  - Do NOT add authentication or CORS
  - Do NOT use a CDN for htmx or CSS — everything vendored in `/static/`

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 3 and 4)
  - **Parallel Group**: Wave 2
  - **Blocks**: Tasks 10, 11, 12, 13, 14
  - **Blocked By**: Task 2 (needs project compiling)

  **References**:

  **Pattern References**:
  - kanidm Axum+htmx+Askama patterns: `HxRequest` extractor detects htmx requests, Askama block fragment rendering (`#[template(path = "page.html", block = "body")]`) serves only the content block for htmx, full page for direct browser requests
  - `askama_web` crate: `#[derive(WebTemplate)]` auto-implements `IntoResponse` for Askama templates
  - `axum-htmx` crate: `HxRequest` extractor, `HxPushUrl`, `HxRequestGuardLayer`
  - `tower-http` fs: `ServeDir::new("static")` for static file serving

  **External References**:
  - Axum docs: https://docs.rs/axum/0.8/
  - askama_web: https://docs.rs/askama_web/
  - axum-htmx: https://docs.rs/axum-htmx/
  - Pico CSS (classless CSS framework): https://picocss.com/

  **WHY Each Reference Matters**:
  - The `HxRequest` extractor is the KEY pattern — same route handler serves full HTML page (browser) or just the content block (htmx partial request). Eliminates duplicate templates.
  - `ServeDir` handles all static file serving with proper MIME types and caching
  - `tracing` + `TraceLayer` provides structured logging without `println!`

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: Server starts and serves home page
    Tool: Bash
    Preconditions: Import completed, cargo compiles
    Steps:
      1. Start server: `cargo run -- serve &`
      2. Wait 2s for startup
      3. Run: `curl -s http://localhost:3000/ -o /tmp/home.html -w "%{http_code}"`
      4. Check status code is 200
      5. Check response contains '<html'
      6. Kill server
    Expected Result: 200 status, valid HTML response
    Failure Indicators: Connection refused, 404, or 500
    Evidence: .sisyphus/evidence/task-8-server-home.txt

  Scenario: Static files served correctly
    Tool: Bash
    Preconditions: Server running
    Steps:
      1. Run: `curl -s http://localhost:3000/static/js/htmx.min.js -o /dev/null -w "%{http_code}"`
      2. Run: `curl -s http://localhost:3000/static/css/style.css -o /dev/null -w "%{http_code}"`
    Expected Result: Both return 200
    Failure Indicators: 404 for static files
    Evidence: .sisyphus/evidence/task-8-static-files.txt

  Scenario: All routes return 200 (placeholder content)
    Tool: Bash
    Preconditions: Server running
    Steps:
      1. Test each route:
         - `curl -s http://localhost:3000/ -w "%{http_code}"` → 200
         - `curl -s http://localhost:3000/search -w "%{http_code}"` → 200
         - `curl -s http://localhost:3000/attachments -w "%{http_code}"` → 200
         - `curl -s http://localhost:3000/analytics -w "%{http_code}"` → 200
    Expected Result: All return 200
    Failure Indicators: Any non-200 status
    Evidence: .sisyphus/evidence/task-8-all-routes.txt
  ```

  **Commit**: YES
  - Message: `feat: add Axum web server with base template and routing`
  - Files: `src/web/mod.rs, src/web/pages.rs, src/web/partials.rs, src/web/attachments.rs, src/state.rs, src/main.rs, templates/base.html, static/css/style.css`

- [ ] 9. Analytics Query Functions

  **What to do**:
  - Add analytics query functions to `src/db/queries.rs`:
    1. `messages_per_conversation(conn, limit) -> Vec<(String, i64)>` — top N conversations by message count
    2. `messages_over_time(conn, granularity: "day"|"week"|"month") -> Vec<(String, i64)>` — message count grouped by time period
    3. `top_contacts(conn, limit) -> Vec<(String, String, i64)>` — (name, handle, message_count) for top contacts
    4. `attachment_stats(conn) -> AttachmentStats` — total count, total size, count by MIME type category (images, videos, audio, documents, other)
    5. `overall_stats(conn) -> OverallStats` — total messages, total conversations, total contacts, total attachments, date range (earliest to latest message)
  - Use straightforward SQL with GROUP BY, COUNT, SUM — no complex analytics
  - Date grouping: use `strftime('%Y-%m-%d', date_unix, 'unixepoch')` for day, `strftime('%Y-%W', ...)` for week, `strftime('%Y-%m', ...)` for month

  **Must NOT do**:
  - Do NOT add DuckDB for analytics
  - Do NOT implement complex statistical analysis
  - Do NOT add chart rendering — that's the UI page's job (Task 14)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 6 and 7)
  - **Parallel Group**: Wave 4
  - **Blocks**: Task 14
  - **Blocked By**: Task 5 (needs messages imported)

  **References**:

  **Pattern References**:
  - SQLite `strftime()` for date grouping: https://www.sqlite.org/lang_datefunc.html
  - Standard SQL aggregation patterns (GROUP BY, COUNT, SUM, ORDER BY)

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: Analytics queries return valid data
    Tool: Bash
    Preconditions: Import completed
    Steps:
      1. Test messages per conversation: `sqlite3 data/imessage.db "SELECT c.display_name, COUNT(*) as cnt FROM messages m JOIN conversations c ON c.id = m.conversation_id GROUP BY m.conversation_id ORDER BY cnt DESC LIMIT 5;"`
      2. Test messages over time: `sqlite3 data/imessage.db "SELECT strftime('%Y-%m', date_unix, 'unixepoch') as month, COUNT(*) FROM messages GROUP BY month ORDER BY month DESC LIMIT 12;"`
      3. Test attachment stats: `sqlite3 data/imessage.db "SELECT CASE WHEN mime_type LIKE 'image%' THEN 'image' WHEN mime_type LIKE 'video%' THEN 'video' ELSE 'other' END as cat, COUNT(*) FROM attachments GROUP BY cat;"`
    Expected Result: All queries return rows with non-zero counts
    Failure Indicators: Empty results or SQL errors
    Evidence: .sisyphus/evidence/task-9-analytics.txt
  ```

  **Commit**: YES
  - Message: `feat: add analytics query functions`
  - Files: `src/db/queries.rs`


- [ ] 10. Conversation List Page

  **What to do**:
  - Implement the `pages::index` handler in `src/web/pages.rs`:
    - Query `db/queries.rs` for all conversations sorted by most recent message date (descending)
    - Each conversation row: display_name (or phone/email if no name), last message preview (truncated to ~80 chars), last message date (relative: "2m ago", "Yesterday", "Mar 15"), unread-style visual weight for recent conversations
    - Use `HxRequest` extractor: if htmx request → render only `content` block; if browser → render full page via `base.html`
  - Create `templates/index.html` extending `base.html`:
    - `{% block content %}` containing conversation list
    - Each conversation is an `<a>` linking to `/conversations/{id}` with `hx-get="/conversations/{id}"` `hx-target="#main"` `hx-push-url="true"`
    - Show message count badge per conversation
    - Search/filter input at top: `hx-get="/partials/conversations"` `hx-trigger="keyup changed delay:300ms"` `hx-target="#conversation-list"` — filters conversation list by name/handle
  - Implement `partials::conversations_partial` in `src/web/partials.rs`:
    - Accepts `q` query param for filtering by contact name or phone/email (LIKE '%q%' on display_name and handle)
    - Returns only the conversation list HTML fragment (no base layout)
    - Apply `HxRequestGuardLayer` on this route — return 403 if accessed directly from browser
  - Add conversation list query to `src/db/queries.rs`:
    ```rust
    pub fn list_conversations(conn: &Connection, filter: Option<&str>) -> Result<Vec<ConversationSummary>>
    // Returns: id, display_name, handle, last_message_preview, last_message_date, message_count
    // SQL: JOIN messages to get latest message per conversation, LEFT JOIN contacts for name
    ```

  **Must NOT do**:
  - Do NOT add real-time updates or WebSocket connections
  - Do NOT implement message preview longer than 80 chars
  - Do NOT add avatar/profile images

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 11, 12, 13, 14)
  - **Parallel Group**: Wave 5
  - **Blocks**: None (except Final Verification)
  - **Blocked By**: Tasks 5, 8 (needs imported data + web server shell)

  **References**:

  **Pattern References**:
  - `src/web/mod.rs` (Task 8) — router structure showing where index route is registered
  - `src/state.rs` (Task 8) — AppState struct with db connection
  - `src/db/queries.rs` (Task 9) — existing query function patterns to follow
  - `templates/base.html` (Task 8) — base layout to extend
  - kanidm `HxRequest` pattern: same handler checks `HxRequest` extractor to decide full page vs partial rendering
  - Askama block fragment: `#[template(path = "index.html", block = "content")]` for partial rendering

  **External References**:
  - htmx `hx-trigger` with delay: https://htmx.org/attributes/hx-trigger/
  - `axum-htmx` `HxRequestGuardLayer`: https://docs.rs/axum-htmx/
  - Askama template inheritance: https://docs.rs/askama/0.13/

  **WHY Each Reference Matters**:
  - The `HxRequest` extractor pattern is essential — one handler, two render paths (full page for browser, fragment for htmx)
  - The conversation filter input uses htmx `delay:300ms` debounce to avoid spamming the server on every keystroke
  - `HxRequestGuardLayer` prevents partial routes from being accessed directly, returning 403

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: Conversation list displays all conversations
    Tool: Bash (curl)
    Preconditions: Import completed, server running
    Steps:
      1. Run: `curl -s http://localhost:3000/ -o /tmp/convos.html -w "%{http_code}"`
      2. Verify status code is 200
      3. Check response contains at least one `<a href="/conversations/` link
      4. Check response contains a display name or phone number
      5. Check response contains a date string
    Expected Result: HTML page with conversation list, each having name + preview + date
    Failure Indicators: Empty list, 500 error, missing conversation data
    Evidence: .sisyphus/evidence/task-10-conversation-list.txt

  Scenario: Conversation list sorted by recency
    Tool: Bash
    Preconditions: Import completed, server running
    Steps:
      1. Run: `curl -s http://localhost:3000/ | grep -oP 'data-date="\K[^"]+' | head -5`
      2. Verify dates are in descending order
    Expected Result: Most recent conversation appears first
    Failure Indicators: Conversations not sorted by date
    Evidence: .sisyphus/evidence/task-10-sort-order.txt

  Scenario: htmx partial returns fragment only
    Tool: Bash
    Preconditions: Server running
    Steps:
      1. Run: `curl -s -H 'HX-Request: true' http://localhost:3000/ -o /tmp/partial.html`
      2. Verify response does NOT contain '<html' or '<head'
      3. Verify response DOES contain conversation list markup
    Expected Result: Fragment HTML without full page wrapper
    Failure Indicators: Full page HTML returned for htmx request
    Evidence: .sisyphus/evidence/task-10-htmx-partial.txt

  Scenario: Conversation filter narrows results
    Tool: Bash
    Preconditions: Server running, at least 2 conversations with different contacts
    Steps:
      1. Run: `curl -s -H 'HX-Request: true' 'http://localhost:3000/partials/conversations?q=KNOWN_CONTACT_NAME'`
      2. Verify response contains the filtered contact
      3. Verify response does NOT contain all conversations
    Expected Result: Filtered list with only matching conversations
    Failure Indicators: Unfiltered list or empty results
    Evidence: .sisyphus/evidence/task-10-filter.txt
  ```

  **Commit**: YES
  - Message: `feat: add conversation list page with htmx`
  - Files: `src/web/pages.rs, src/web/partials.rs, src/db/queries.rs, templates/index.html`

- [ ] 11. Message Viewer Page with Infinite Scroll Pagination

  **What to do**:
  - Implement the `pages::conversation` handler in `src/web/pages.rs`:
    - Route: `/conversations/{id}` — loads a specific conversation
    - Query first 50 messages ordered by date descending (newest first at top)
    - Include conversation header: contact display_name (or handle), total message count
    - Distinguish sent vs received messages (check `is_from_me` field) with visual styling (different alignment/color)
    - Format each message: sender indicator, message body, date/time, `is_read` status
    - Handle `HxRequest` for full page vs partial rendering
  - Create `templates/conversation.html` extending `base.html`:
    - Conversation header with contact name and back link to conversation list
    - Message list container `<div id="messages">`
    - Each message in a bubble-style layout (sent = right-aligned, received = left-aligned)
    - Attachment indicators: if message has attachments, show filename/type as clickable link to `/attachments/download/{id}`
    - Timestamps formatted as readable local time ("Mar 15, 2024 2:30 PM")
  - Create `templates/components/message_row.html`:
    - Reusable message row template used by both full page and partial
    - `is_from_me` → different CSS class for alignment
    - Attachment badge if `attachment_count > 0`
  - Implement infinite scroll pagination via htmx:
    - The LAST message in each page has: `hx-get="/partials/messages?conversation_id={id}&before={oldest_date_unix}"` `hx-trigger="revealed"` `hx-swap="afterend"`
    - `partials::messages_partial` in `src/web/partials.rs`:
      - Accepts `conversation_id` + `before` (unix timestamp) query params
      - Returns next 50 messages older than `before` timestamp
      - Returns empty body when no more messages (stops infinite scroll)
      - Apply `HxRequestGuardLayer` — htmx-only route
  - Add message query functions to `src/db/queries.rs`:
    ```rust
    pub fn get_messages(conn: &Connection, conversation_id: i64, before: Option<i64>, limit: i64) -> Result<Vec<MessageRow>>
    // Returns: id, body, date_unix, is_from_me, sender_name, attachment_count
    // SQL: WHERE conversation_id = ? AND (date_unix < ? OR ? IS NULL) ORDER BY date_unix DESC LIMIT ?
    ```

  **Must NOT do**:
  - Do NOT implement message editing or deletion
  - Do NOT add real-time message updates
  - Do NOT implement thread/reply views (v1 shows flat list only)
  - Do NOT load more than 50 messages per request

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 10, 12, 13, 14)
  - **Parallel Group**: Wave 5
  - **Blocks**: None (except Final Verification)
  - **Blocked By**: Tasks 5, 8 (needs imported data + web server shell)

  **References**:

  **Pattern References**:
  - `src/web/mod.rs` (Task 8) — conversation route registration: `.route("/conversations/{id}", get(pages::conversation))`
  - `src/web/pages.rs` (Task 8) — placeholder handler to replace with real implementation
  - `src/db/queries.rs` (Tasks 5, 9) — existing query patterns
  - `templates/base.html` (Task 8) — layout to extend
  - `src/models/mod.rs` (Task 3) — `MessageRow` struct with `is_from_me`, `body`, `date_unix` fields

  **External References**:
  - htmx infinite scroll pattern: `hx-trigger="revealed"` on last element, `hx-swap="afterend"` — https://htmx.org/examples/infinite-scroll/
  - Askama block fragment rendering for partials

  **WHY Each Reference Matters**:
  - The `revealed` trigger fires when element scrolls into viewport — perfect for "load more" without a button
  - `hx-swap="afterend"` appends new messages after the trigger element, maintaining scroll position
  - Empty response body from partial = htmx stops polling = infinite scroll terminates naturally

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: Conversation page loads with messages
    Tool: Bash (curl)
    Preconditions: Import completed, server running, at least one conversation with messages
    Steps:
      1. Get first conversation ID: `sqlite3 data/imessage.db "SELECT id FROM conversations LIMIT 1;"`
      2. Run: `curl -s http://localhost:3000/conversations/{id} -o /tmp/convo.html -w "%{http_code}"`
      3. Verify status 200
      4. Verify response contains message text content
      5. Verify response contains contact name or handle in header
    Expected Result: Full page with conversation header + first 50 messages
    Failure Indicators: 404, 500, empty message list
    Evidence: .sisyphus/evidence/task-11-conversation-page.txt

  Scenario: Messages distinguish sent vs received
    Tool: Bash
    Preconditions: Server running, conversation with both sent and received messages
    Steps:
      1. Fetch conversation page
      2. Check for presence of both CSS classes: `class="sent"` and `class="received"` (or equivalent)
    Expected Result: Different styling classes applied to sent vs received messages
    Failure Indicators: All messages have same class
    Evidence: .sisyphus/evidence/task-11-sent-received.txt

  Scenario: Infinite scroll pagination loads more messages
    Tool: Bash (curl)
    Preconditions: Conversation with >50 messages
    Steps:
      1. Get conversation ID with most messages
      2. Fetch first page: `curl -s http://localhost:3000/conversations/{id}`
      3. Extract oldest date_unix from the response
      4. Fetch next page: `curl -s -H 'HX-Request: true' 'http://localhost:3000/partials/messages?conversation_id={id}&before={oldest_date}'`
      5. Verify response contains additional messages
      6. Verify messages are older than the `before` timestamp
    Expected Result: Additional batch of messages older than first page's oldest
    Failure Indicators: Empty response when more messages exist, or messages not older than cutoff
    Evidence: .sisyphus/evidence/task-11-pagination.txt

  Scenario: Pagination terminates when no more messages
    Tool: Bash
    Preconditions: Server running
    Steps:
      1. Fetch messages partial with `before=0` (before epoch — no messages should exist)
      2. Verify response body is empty
    Expected Result: Empty response body (signals htmx to stop infinite scroll)
    Failure Indicators: Non-empty response or error
    Evidence: .sisyphus/evidence/task-11-pagination-end.txt
  ```

  **Commit**: YES
  - Message: `feat: add message viewer with infinite scroll pagination`
  - Files: `src/web/pages.rs, src/web/partials.rs, src/db/queries.rs, templates/conversation.html, templates/components/message_row.html`

- [ ] 12. Search Page with FTS5 Results and Highlighting

  **What to do**:
  - Implement the `pages::search` handler in `src/web/pages.rs`:
    - Route: `/search` — accepts `q` query param
    - If `q` is empty or absent: render search page with empty state (just the search input)
    - If `q` has 1-2 chars: use LIKE fallback (`WHERE body LIKE '%q%'`), return results with manual highlighting
    - If `q` has 3+ chars: use FTS5 `MATCH` with `highlight(messages_fts, 0, '<mark>', '</mark>')` for result highlighting
    - Results include: highlighted message body snippet, conversation name, sender, date, link to conversation context
    - Paginate results: 20 per page, with "Load More" via htmx
    - Handle `HxRequest` for full page vs partial
  - Create `templates/search.html` extending `base.html`:
    - Search input: `<input type="search" name="q" hx-get="/partials/search-results" hx-trigger="keyup changed delay:300ms" hx-target="#results" hx-include="this">`
    - Results container `<div id="results">`
    - Each result: highlighted body with `<mark>` tags (rendered as safe/unescaped HTML in Askama: `{{ highlighted_body|safe }}`), conversation name as link, sender, relative date
    - Result count header: "N results for 'query'"
    - Empty state: "No results found for 'query'"
    - Loading indicator via htmx `hx-indicator`
  - Implement `partials::search_results_partial` in `src/web/partials.rs`:
    - Accepts `q` (search term) + `offset` (pagination) query params
    - Returns search result HTML fragment
    - Apply `HxRequestGuardLayer`
  - Add search query function to `src/db/queries.rs`:
    ```rust
    pub fn search_messages(conn: &Connection, query: &str, offset: i64, limit: i64) -> Result<(Vec<SearchResult>, i64)>
    // Returns: (results, total_count)
    // SearchResult: message_id, highlighted_body, conversation_id, conversation_name, sender_name, date_unix
    // For query.len() >= 3: FTS5 MATCH with highlight()
    // For query.len() < 3: LIKE '%query%' with manual <mark> wrapping
    ```
  - Sanitize search input: escape FTS5 special characters (`"`, `*`, `NEAR`, `OR`, `AND`, `NOT`) by wrapping user query in double quotes for FTS5

  **Must NOT do**:
  - Do NOT implement advanced search syntax (AND/OR/NOT operators for user) — always wrap in quotes
  - Do NOT add search history or saved searches
  - Do NOT add fuzzy matching beyond what FTS5 trigram provides

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 10, 11, 13, 14)
  - **Parallel Group**: Wave 5
  - **Blocks**: None (except Final Verification)
  - **Blocked By**: Tasks 7, 8 (needs FTS5 search module + web server shell)

  **References**:

  **Pattern References**:
  - `src/search/mod.rs` (Task 7) — search logic with FTS5 and LIKE fallback
  - `src/web/mod.rs` (Task 8) — search route registration
  - `src/db/queries.rs` — query patterns
  - `templates/base.html` — layout

  **External References**:
  - FTS5 `highlight()` function: https://www.sqlite.org/fts5.html#the_highlight_function
  - htmx active search pattern: https://htmx.org/examples/active-search/
  - Askama `|safe` filter for unescaped HTML

  **WHY Each Reference Matters**:
  - FTS5 `highlight()` returns body text with `<mark>` tags around matches — must render as `|safe` in Askama to avoid double-escaping
  - The htmx active search pattern (keyup + delay) provides real-time search-as-you-type UX
  - FTS5 special chars must be escaped to prevent SQL injection via search input — wrapping in double quotes is the simplest safe approach

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: Search returns highlighted results for 3+ char query
    Tool: Bash (curl)
    Preconditions: Import completed with FTS5 index, server running
    Steps:
      1. Pick a word known to exist in messages (e.g., from: `sqlite3 data/imessage.db "SELECT body FROM messages WHERE body IS NOT NULL LIMIT 1;"`)
      2. Run: `curl -s 'http://localhost:3000/search?q=KNOWN_WORD' -o /tmp/search.html -w "%{http_code}"`
      3. Verify status 200
      4. Verify response contains '<mark>KNOWN_WORD</mark>' (highlighted match)
      5. Verify response contains result count
      6. Verify response contains conversation name link
    Expected Result: Search results with highlighted matching text
    Failure Indicators: No results, missing highlighting, 500 error
    Evidence: .sisyphus/evidence/task-12-search-results.txt

  Scenario: Short query (<3 chars) uses LIKE fallback
    Tool: Bash
    Preconditions: Server running
    Steps:
      1. Run: `curl -s 'http://localhost:3000/search?q=hi' -o /tmp/short.html -w "%{http_code}"`
      2. Verify status 200
      3. Verify response contains results (if 'hi' appears in any messages)
    Expected Result: Results found via LIKE fallback, no FTS5 error
    Failure Indicators: FTS5 error for short query, empty results when data exists
    Evidence: .sisyphus/evidence/task-12-short-query.txt

  Scenario: Empty query shows empty state
    Tool: Bash
    Preconditions: Server running
    Steps:
      1. Run: `curl -s 'http://localhost:3000/search' -o /tmp/empty.html`
      2. Verify page renders (200 status)
      3. Verify search input is present
      4. Verify no results section is shown
    Expected Result: Clean search page with input, no results displayed
    Failure Indicators: Error page, results shown without query
    Evidence: .sisyphus/evidence/task-12-empty-state.txt

  Scenario: Search results link to conversation context
    Tool: Bash
    Preconditions: Server running, search returns results
    Steps:
      1. Fetch search results for a known query
      2. Verify each result contains an `<a href="/conversations/` link
    Expected Result: Clickable links from search results to conversation viewer
    Failure Indicators: Missing conversation links
    Evidence: .sisyphus/evidence/task-12-context-links.txt

  Scenario: FTS5 special characters don't cause errors
    Tool: Bash
    Preconditions: Server running
    Steps:
      1. Run: `curl -s 'http://localhost:3000/search?q=hello+OR+world' -w "%{http_code}"`
      2. Run: `curl -s 'http://localhost:3000/search?q=%22quoted%22' -w "%{http_code}"`
      3. Verify both return 200 (not 500)
    Expected Result: Special characters safely escaped, no SQL errors
    Failure Indicators: 500 error from unescaped FTS5 operators
    Evidence: .sisyphus/evidence/task-12-special-chars.txt
  ```

  **Commit**: YES
  - Message: `feat: add search page with FTS5 results and highlighting`
  - Files: `src/web/pages.rs, src/web/partials.rs, src/db/queries.rs, templates/search.html`

- [ ] 13. Attachment Browser Page and File Download Serving

  **What to do**:
  - Implement the `pages::attachments_page` handler in `src/web/pages.rs`:
    - Route: `/attachments` — accepts optional `mime` query param for filtering
    - Query all attachments with metadata: filename, mime_type, file_size, associated conversation name, associated message date, file_exists (boolean)
    - Default view: grid of attachment thumbnails (images) / file icons (non-images), grouped by type
    - Filter tabs/buttons: All, Images, Videos, Audio, Documents — each sets `mime` param via htmx
    - Show file size in human-readable format (KB/MB/GB)
    - Show associated conversation name and date as context
    - Handle `HxRequest` for full page vs partial
  - Create `templates/attachments.html` extending `base.html`:
    - Filter buttons at top: `hx-get="/partials/attachments?mime=image"` `hx-target="#attachment-grid"` `hx-push-url="true"` with `hx-swap="innerHTML"`
    - Attachment grid/list container `<div id="attachment-grid">`
    - Each attachment card: filename, file size, MIME type icon/badge, download link, conversation context link
    - Download link: `<a href="/attachments/download/{id}">` for each attachment
    - Visual indicator for missing files (file_exists = false): greyed out with "File not found" label
  - Implement `web::attachments::download` handler in `src/web/attachments.rs`:
    - Route: `/attachments/download/{id}` — serves the actual file
    - Look up attachment by ID in database to get `resolved_path`
    - Verify file exists at `resolved_path` — return 404 with clear message if not
    - Use `axum_extra::body::FileStream` for streaming large files (videos can be 100MB+)
    - Set `Content-Type` from stored `mime_type`
    - Set `Content-Disposition: attachment; filename="original_filename"` for download
  - Implement `partials::attachments_partial` in `src/web/partials.rs` (optional — for filter without page reload):
    - Accepts `mime` query param
    - Returns filtered attachment grid fragment
    - Apply `HxRequestGuardLayer`
  - Add attachment query functions to `src/db/queries.rs`:
    ```rust
    pub fn list_attachments(conn: &Connection, mime_filter: Option<&str>, offset: i64, limit: i64) -> Result<Vec<AttachmentRow>>
    // Returns: id, filename, mime_type, file_size, resolved_path, file_exists, conversation_name, message_date
    // mime_filter: None = all, "image" = WHERE mime_type LIKE 'image/%', etc.
    pub fn get_attachment(conn: &Connection, id: i64) -> Result<Option<AttachmentRow>>
    ```

  **Must NOT do**:
  - Do NOT generate thumbnails or image previews — just show icons/file info
  - Do NOT implement inline video/audio playback in v1
  - Do NOT re-encode or transform files — serve as-is
  - Do NOT implement bulk download (zip)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 10, 11, 12, 14)
  - **Parallel Group**: Wave 5
  - **Blocks**: None (except Final Verification)
  - **Blocked By**: Tasks 6, 8 (needs attachment metadata imported + web server shell)

  **References**:

  **Pattern References**:
  - `src/web/mod.rs` (Task 8) — attachment routes: `.route("/attachments", get(pages::attachments_page))`, `.route("/attachments/download/{id}", get(attachments::download))`
  - `src/import/attachments.rs` (Task 6) — attachment import with `resolved_path` and `file_exists` fields
  - `src/db/queries.rs` — query patterns
  - `src/models/mod.rs` (Task 3) — `AttachmentRow` struct

  **External References**:
  - `axum_extra::body::FileStream`: https://docs.rs/axum-extra/0.10/axum_extra/body/struct.FileStream.html
  - `Content-Disposition` header for downloads

  **WHY Each Reference Matters**:
  - `FileStream` is critical for large files (videos) — streams the file instead of loading entirely into memory
  - The `resolved_path` from import already has `~` expanded to absolute path and `file_exists` pre-checked
  - MIME type filtering uses `LIKE 'image/%'` etc. to match all subtypes within a category

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: Attachment browser lists all attachments
    Tool: Bash (curl)
    Preconditions: Import completed with attachments, server running
    Steps:
      1. Run: `curl -s http://localhost:3000/attachments -o /tmp/attachments.html -w "%{http_code}"`
      2. Verify status 200
      3. Verify response contains attachment filenames
      4. Verify response contains download links (`/attachments/download/`)
    Expected Result: Page listing attachments with metadata and download links
    Failure Indicators: Empty list, 500 error, missing download links
    Evidence: .sisyphus/evidence/task-13-attachment-list.txt

  Scenario: MIME type filter narrows results
    Tool: Bash
    Preconditions: Server running, attachments include both images and non-images
    Steps:
      1. Run: `curl -s 'http://localhost:3000/attachments?mime=image' -o /tmp/images.html`
      2. Verify all shown attachments have image MIME types
      3. Run: `curl -s 'http://localhost:3000/attachments?mime=video' -o /tmp/videos.html`
      4. Verify results differ from image filter
    Expected Result: Filtered attachment list matching requested MIME category
    Failure Indicators: Unfiltered results, or empty results when matching attachments exist
    Evidence: .sisyphus/evidence/task-13-mime-filter.txt

  Scenario: Attachment download serves correct file
    Tool: Bash
    Preconditions: Server running, at least one attachment with existing file
    Steps:
      1. Get an attachment ID with existing file: `sqlite3 data/imessage.db "SELECT id FROM attachments WHERE file_exists = 1 LIMIT 1;"`
      2. Run: `curl -s http://localhost:3000/attachments/download/{id} -o /tmp/download_test -w "%{http_code}\n%{content_type}"`
      3. Verify status 200
      4. Verify Content-Type matches stored mime_type
      5. Verify file size > 0
    Expected Result: File downloaded with correct MIME type and non-zero size
    Failure Indicators: 404, wrong content type, empty file
    Evidence: .sisyphus/evidence/task-13-download.txt

  Scenario: Missing file returns 404
    Tool: Bash
    Preconditions: Server running
    Steps:
      1. Get an attachment ID with missing file: `sqlite3 data/imessage.db "SELECT id FROM attachments WHERE file_exists = 0 LIMIT 1;"`
      2. Run: `curl -s http://localhost:3000/attachments/download/{id} -w "%{http_code}"`
      3. Verify status 404
    Expected Result: 404 status for non-existent file
    Failure Indicators: 500 error or 200 with empty body
    Evidence: .sisyphus/evidence/task-13-missing-file.txt
  ```

  **Commit**: YES
  - Message: `feat: add attachment browser and file download serving`
  - Files: `src/web/pages.rs, src/web/partials.rs, src/web/attachments.rs, src/db/queries.rs, templates/attachments.html`

- [ ] 14. Analytics Dashboard Page

  **What to do**:
  - Implement the `pages::analytics` handler in `src/web/pages.rs`:
    - Route: `/analytics` — no query params needed
    - Call all analytics query functions from Task 9 (messages_per_conversation, messages_over_time, top_contacts, attachment_stats, overall_stats)
    - Handle `HxRequest` for full page vs partial
  - Create `templates/analytics.html` extending `base.html`:
    - **Overview Section** (overall_stats):
      - Total messages, total conversations, total contacts, total attachments
      - Date range: "Your messages span from {earliest} to {latest}"
      - Display as a simple stats grid (4 cards with large numbers)
    - **Top Conversations** (messages_per_conversation):
      - HTML table: Rank, Conversation Name, Message Count
      - Top 10 conversations by message count
      - Each name links to `/conversations/{id}`
    - **Top Contacts** (top_contacts):
      - HTML table: Rank, Contact Name, Handle, Message Count
      - Top 10 contacts
    - **Messages Over Time** (messages_over_time):
      - Simple HTML bar chart using CSS width percentages (no JS chart library for v1):
        ```html
        <div class="bar" style="width: {{ percentage }}%">{{ count }}</div>
        ```
      - Show last 12 months by default
    - **Attachment Breakdown** (attachment_stats):
      - Simple list: Images: N, Videos: N, Audio: N, Documents: N, Other: N, Total size: X GB

  **Must NOT do**:
  - Do NOT add Chart.js or any JS charting library — CSS-only bar charts
  - Do NOT implement date range filtering for analytics (v1 shows all-time)
  - Do NOT add export/download of analytics data
  - Do NOT implement real-time updating analytics

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 10, 11, 12, 13)
  - **Parallel Group**: Wave 5
  - **Blocks**: None (except Final Verification)
  - **Blocked By**: Tasks 8, 9 (needs web server shell + analytics queries)

  **References**:

  **Pattern References**:
  - `src/db/queries.rs` (Task 9) — all analytics query functions (messages_per_conversation, messages_over_time, top_contacts, attachment_stats, overall_stats)
  - `src/web/mod.rs` (Task 8) — analytics route registration
  - `templates/base.html` (Task 8) — layout to extend

  **External References**:
  - CSS-only bar charts: use `width: calc(percentage * 1%)` or inline styles
  - Pico CSS tables for clean styling without extra classes

  **WHY Each Reference Matters**:
  - The analytics query functions from Task 9 return all the data needed — this task is purely presentation/template work
  - CSS bar charts avoid any JS dependency while providing visual data representation

  **Acceptance Criteria**:

  **QA Scenarios:**

  ```
  Scenario: Analytics page loads with all sections
    Tool: Bash (curl)
    Preconditions: Import completed, server running
    Steps:
      1. Run: `curl -s http://localhost:3000/analytics -o /tmp/analytics.html -w "%{http_code}"`
      2. Verify status 200
      3. Verify response contains "Total Messages" or similar stat heading
      4. Verify response contains a table with conversation names
      5. Verify response contains attachment breakdown section
      6. Verify response contains messages-over-time section
    Expected Result: Full analytics page with all 5 sections populated
    Failure Indicators: Missing sections, 500 error, zero values when data exists
    Evidence: .sisyphus/evidence/task-14-analytics-page.txt

  Scenario: Analytics stats match actual data
    Tool: Bash
    Preconditions: Import completed, server running
    Steps:
      1. Get actual message count: `sqlite3 data/imessage.db "SELECT COUNT(*) FROM messages;"`
      2. Fetch analytics page and extract displayed total message count
      3. Compare: displayed count should match DB count
    Expected Result: Displayed statistics match underlying database counts
    Failure Indicators: Mismatch between displayed and actual counts
    Evidence: .sisyphus/evidence/task-14-stats-accuracy.txt

  Scenario: Top conversations link to conversation viewer
    Tool: Bash
    Preconditions: Server running
    Steps:
      1. Fetch analytics page
      2. Check that conversation names in the top conversations table are wrapped in `<a href="/conversations/` links
    Expected Result: Clickable links from analytics to conversation viewer
    Failure Indicators: Plain text names without links
    Evidence: .sisyphus/evidence/task-14-conversation-links.txt
  ```

  **Commit**: YES
  - Message: `feat: add analytics dashboard page`
  - Files: `src/web/pages.rs, templates/analytics.html`
---

## Final Verification Wave

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists (read file, curl endpoint, run command). For each "Must NOT Have": search codebase for forbidden patterns — reject with file:line if found. Check evidence files exist in .sisyphus/evidence/. Compare deliverables against plan.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo check`, `cargo clippy`. Review all source files for: `unwrap()` in non-test code without justification, empty error handlers, `println!` instead of `tracing`, dead code, unused imports. Check for AI slop: excessive comments, over-abstraction, generic variable names (data/result/item/temp), unnecessary trait implementations.
  Output: `Build [PASS/FAIL] | Clippy [PASS/FAIL] | Files [N clean/N issues] | VERDICT`

- [ ] F3. **Real Manual QA** — `unspecified-high` + `playwright` skill
  Start server (`cargo run -- serve`). Using Playwright: navigate to every page (home, conversations, messages, search, attachments, analytics). Test search with real query terms. Test attachment browser filters. Test conversation pagination (infinite scroll). Test with empty search, short query (<3 chars), long query. Screenshot every page. Save to `.sisyphus/evidence/final-qa/`.
  Output: `Pages [N/N pass] | Search [N/N] | Edge Cases [N tested] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual source files. Verify 1:1 — everything in spec was built (no missing), nothing beyond spec was built (no creep). Check "Must NOT Have" compliance (no Tantivy, no workspace, no CDN, no second FTS table, no sync triggers). Flag unaccounted files.
  Output: `Tasks [N/N compliant] | Scope [CLEAN/N issues] | Unaccounted [CLEAN/N files] | VERDICT`

---

## Commit Strategy

| After Task(s) | Message | Files |
|---------------|---------|-------|
| 1 | `feat: scaffold project with Cargo.toml and module stubs` | Cargo.toml, src/**, static/**, templates/** |
| 2 | `feat: verify imessage-database reads chat.db (proof of concept)` | src/main.rs |
| 3 | `feat: add database schema creation and migration` | src/db/** |
| 4 | `feat: add AddressBook contact resolution` | src/import/contacts.rs |
| 5 | `feat: implement full message import pipeline with typedstream decoding` | src/import/** |
| 6 | `feat: import attachment metadata from chat.db` | src/import/attachments.rs |
| 7 | `feat: add FTS5 trigram search with LIKE fallback` | src/search/** |
| 8 | `feat: add Axum web server with base template and routing` | src/web/**, templates/base.html |
| 9 | `feat: add analytics query functions` | src/db/queries.rs |
| 10 | `feat: add conversation list page with htmx` | src/web/pages.rs, templates/index.html |
| 11 | `feat: add message viewer with infinite scroll pagination` | src/web/pages.rs, templates/conversation.html |
| 12 | `feat: add search page with FTS5 results and highlighting` | src/web/pages.rs, templates/search.html |
| 13 | `feat: add attachment browser and file download serving` | src/web/attachments.rs, templates/attachments.html |
| 14 | `feat: add analytics dashboard page` | src/web/pages.rs, templates/analytics.html |

---

## Success Criteria

### Verification Commands
```bash
# Build
cargo check                          # Expected: no errors
cargo clippy                         # Expected: no warnings

# Import
cargo run -- import                  # Expected: completes with progress bar, no errors
sqlite3 data/imessage.db "SELECT COUNT(*) FROM messages;"  # Expected: > 0

# FTS5 search
sqlite3 data/imessage.db "SELECT COUNT(*) FROM messages_fts WHERE messages_fts MATCH 'hello';"  # Expected: > 0

# Web server
cargo run -- serve &
curl -s http://localhost:3000/ -o /dev/null -w "%{http_code}"                    # Expected: 200
curl -s http://localhost:3000/search?q=hello -o /dev/null -w "%{http_code}"      # Expected: 200
curl -s -o /dev/null -w "%{time_total}" "http://localhost:3000/search?q=hello"   # Expected: < 0.2

# Short query fallback
curl -s http://localhost:3000/search?q=hi -o /dev/null -w "%{http_code}"         # Expected: 200
```

### Final Checklist
- [ ] All "Must Have" features present and working
- [ ] All "Must NOT Have" items absent from codebase
- [ ] `cargo check` and `cargo clippy` pass clean
- [ ] Import handles real chat.db with 100K+ messages
- [ ] Search returns results in <200ms
- [ ] All web pages render correctly
- [ ] Attachment downloads work for existing files
