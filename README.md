# imessage-db-port

`imessage-db-port` imports data from the macOS Messages database into a local SQLite database and serves a web UI for browsing conversations, searching messages, and recovering attachments.

It is built for local, personal use: import your data once, then explore it in the browser without needing to query the Apple database directly.

## What it does

- Imports conversations, messages, contacts, and attachments from macOS Messages data
- Builds a local SQLite database at `data/imessage.db`
- Serves a browser UI for conversation browsing and full-text message search
- Lets you inspect and download attachments, including image/video previews
- Includes tools to scan iOS backups and repair missing attachment metadata

## Requirements

- macOS with access to your Messages data
- Rust toolchain installed (`cargo`)

The app reads from your local Messages database and writes its own local database in this repo.

## Quick Start

1. Build the project:

```bash
cargo build
```

2. Import your iMessage data into the local SQLite database:

```bash
cargo run -- import
```

3. Start the web server:

```bash
cargo run -- serve
```

4. Open the app in your browser:

```text
http://127.0.0.1:3000
```

## Common Commands

### Import data

Incremental import:

```bash
cargo run -- import
```

Full reimport that drops and recreates tables first:

```bash
cargo run -- import --full
```

### Run the web app

```bash
cargo run -- serve
```

The server listens on `0.0.0.0:3000`.

### Scan an iOS backup for missing attachments

```bash
cargo run -- scan-ios-backup --backup-path <dir>
```

Copy recovered files into local storage while scanning:

```bash
cargo run -- scan-ios-backup --backup-path <dir> --copy
```

### Repair attachment availability metadata

```bash
cargo run -- repair-attachments
```

Rescan a backup and optionally copy recovered files:

```bash
cargo run -- repair-attachments --backup-path <dir> --copy
```

## Using the Web UI

After importing and starting the server, you can:

- Browse conversation threads from the home page
- Open a conversation and inspect message history
- Search messages from the search page
- Browse attachments by type and sync status
- Preview supported image and video attachments in the browser
- Open contact insight pages and conversation details
- Review missing attachments from the recovery page

## Data Location

- Local app database: `data/imessage.db`
- Static assets: `static/`
- HTML templates: `templates/`

The original Apple Messages database is not modified by this project. Imports populate the local SQLite database used by the web app.

## Development Commands

Run tests:

```bash
cargo test
```

Run lints:

```bash
cargo clippy
```

Format the code:

```bash
cargo fmt
```

## Tech Stack

- Rust
- Axum
- Askama
- SQLite via `rusqlite`

## Notes

- Run `import` before `serve`, or the server will fail because `data/imessage.db` does not exist yet.
- Attachment recovery commands are useful when message records exist but original files are no longer available locally.
