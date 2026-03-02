use clap::{Parser, Subcommand};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

mod db;
mod error;
mod import;
mod models;
mod search;
mod state;
mod web;

#[derive(Parser)]
#[command(name = "imessage-db-port", about = "iMessage database viewer")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Import iMessage data into the local database
    Import,
    /// Start the web server
    Serve,
}

fn serve() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let db_path = PathBuf::from("data/imessage.db");
    if !db_path.exists() {
        anyhow::bail!("Database not found at {db_path:?}. Run 'import' first.");
    }

    let conn = Connection::open(&db_path)?;
    let home = std::env::var("HOME").unwrap_or_default();
    let attachment_root = PathBuf::from(home).join("Library/Messages/Attachments");

    let state = state::AppState {
        db: Arc::new(Mutex::new(conn)),
        attachment_root,
    };

    let app = web::router().with_state(state);

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
        tracing::info!("listening on http://0.0.0.0:3000");
        axum::serve(listener, app).await?;
        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Import => {
            if let Err(err) = import::run_import() {
                eprintln!("import failed: {err}");
                std::process::exit(1);
            }
        }
        Commands::Serve => {
            if let Err(err) = serve() {
                eprintln!("serve failed: {err}");
                std::process::exit(1);
            }
        }
    }
}
