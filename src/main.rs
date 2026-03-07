use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

mod db;
mod error;
mod import;
mod recovery;
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
    Import {
        #[arg(long, help = "Force a full reimport (drop and recreate all tables)")]
        full: bool,
    },
    /// Start the web server
    Serve,
    /// Scan iOS backup for missing attachments
    ScanIosBackup {
        #[arg(long, help = "Path to iOS backup directory")]
        backup_path: PathBuf,
        #[arg(long, help = "Copy found files to local storage")]
        copy: bool,
    },
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
    let state = state::AppState {
        db: Arc::new(Mutex::new(conn)),
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

fn scan_ios_backup(backup_path: &Path, copy: bool) -> anyhow::Result<()> {
    let db_path = PathBuf::from("data/imessage.db");
    if !db_path.exists() {
        anyhow::bail!("Database not found at {db_path:?}. Run 'import' first.");
    }

    let conn = Connection::open(&db_path)?;
    let missing = db::queries::get_missing_attachments(&conn, 0, i64::MAX)?;

    if missing.is_empty() {
        println!("No missing attachments found.");
        return Ok(());
    }

    println!("Found {} missing attachments. Scanning iOS backup...", missing.len());

    let pb = ProgressBar::new(missing.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} ({eta})")?
            .progress_chars("=>-"),
    );

    let mut found = 0u64;
    let mut copied = 0u64;
    let mut errors = 0u64;

    for att in &missing {
        let filename = att.filename.as_deref().unwrap_or("");
        if filename.is_empty() {
            pb.inc(1);
            continue;
        }

        if let Some(backup_file) = recovery::ios_backup::scan_for_attachment(backup_path, filename) {
            let backup_str = backup_file.to_string_lossy().to_string();
            if let Err(e) = db::queries::update_attachment_backup_source(&conn, att.id, &backup_str) {
                pb.println(format!("Failed to update DB for attachment {}: {e}", att.id));
                errors += 1;
            } else {
                found += 1;
            }

            if copy {
                if let Some(resolved) = &att.resolved_path {
                    let dst = PathBuf::from(resolved);
                    match recovery::ios_backup::copy_from_backup(&backup_file, &dst) {
                        Ok(_) => copied += 1,
                        Err(e) => {
                            pb.println(format!("Failed to copy {}: {e}", att.display_name()));
                            errors += 1;
                        }
                    }
                }
            }
        }

        pb.inc(1);
    }

    pb.finish_with_message("done");
    println!();
    println!("Scan complete:");
    println!("  Total missing:  {}", missing.len());
    println!("  Found in backup: {found}");
    if copy {
        println!("  Copied:          {copied}");
    }
    if errors > 0 {
        println!("  Errors:          {errors}");
    }

    Ok(())
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Import { full } => {
            if let Err(err) = import::run_import(full) {
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
        Commands::ScanIosBackup { backup_path, copy } => {
            if let Err(err) = scan_ios_backup(&backup_path, copy) {
                eprintln!("scan-ios-backup failed: {err}");
                std::process::exit(1);
            }
        }
    }
}
