use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
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
    /// Repair attachment availability metadata and optionally rescan backup
    RepairAttachments {
        #[arg(
            long,
            help = "Path to iOS backup directory used to backfill missing files"
        )]
        backup_path: Option<PathBuf>,
        #[arg(long, help = "Copy found backup files to local storage")]
        copy: bool,
    },
}

fn serve() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let db_path = PathBuf::from("data/imessage.db");
    if !db_path.exists() {
        anyhow::bail!("Database not found at {db_path:?}. Run 'import' first.");
    }

    let conn = db::open_existing(&db_path)?;
    let state = state::AppState {
        db: Arc::new(Mutex::new(conn)),
        conversation_insights_cache: Arc::new(RwLock::new(HashMap::new())),
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

    let conn = db::open_existing(&db_path)?;
    let missing = db::queries::get_missing_attachments(&conn, 0, i64::MAX)?;

    if missing.is_empty() {
        println!("No missing attachments found.");
        return Ok(());
    }

    println!(
        "Found {} missing attachments. Scanning iOS backup...",
        missing.len()
    );

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

        if let Some(backup_file) = recovery::ios_backup::scan_for_attachment(backup_path, filename)
        {
            let backup_str = backup_file.to_string_lossy().to_string();
            if let Err(e) = db::queries::update_attachment_backup_source(&conn, att.id, &backup_str)
            {
                pb.println(format!(
                    "Failed to update DB for attachment {}: {e}",
                    att.id
                ));
                errors += 1;
            } else {
                found += 1;
            }

            if copy {
                if let Some(resolved) = &att.resolved_path {
                    let dst = PathBuf::from(resolved);
                    match recovery::ios_backup::copy_from_backup(&backup_file, &dst) {
                        Ok(_) => {
                            if let Err(e) = db::queries::update_attachment_availability(
                                &conn,
                                att.id,
                                Some(resolved),
                                true,
                                Some(&backup_str),
                            ) {
                                pb.println(format!(
                                    "Failed to mark copied attachment {} as present: {e}",
                                    att.id
                                ));
                                errors += 1;
                            } else {
                                copied += 1;
                            }
                        }
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

fn attachment_local_state(att: &db::queries::AttachmentRow, home: &str) -> (Option<String>, bool) {
    let (resolved_from_filename, exists_from_filename) =
        import::attachments::resolve_path(att.filename.as_deref(), home);

    if resolved_from_filename.is_some() {
        return (resolved_from_filename, exists_from_filename);
    }

    match att.resolved_path.as_deref() {
        Some(path) if !path.is_empty() => {
            let exists = Path::new(path).exists();
            (Some(path.to_string()), exists)
        }
        _ => (None, false),
    }
}

fn repair_attachments(backup_path: Option<&Path>, copy: bool) -> anyhow::Result<()> {
    if copy && backup_path.is_none() {
        anyhow::bail!("--copy requires --backup-path");
    }

    let db_path = PathBuf::from("data/imessage.db");
    if !db_path.exists() {
        anyhow::bail!("Database not found at {db_path:?}. Run 'import' first.");
    }

    let conn = db::open_existing(&db_path)?;
    let attachments = db::queries::all_attachments_for_repair(&conn)?;

    if attachments.is_empty() {
        println!("No attachments found.");
        return Ok(());
    }

    let home = std::env::var("HOME").unwrap_or_default();
    let pb = ProgressBar::new(attachments.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} ({eta})")?
            .progress_chars("=>-"),
    );

    let mut updated = 0u64;
    let mut local_present = 0u64;
    let mut backup_found = 0u64;
    let mut copied = 0u64;
    let mut errors = 0u64;

    for att in &attachments {
        let (resolved_path, mut file_exists) = attachment_local_state(att, &home);
        let mut backup_source_path = att.backup_source_path.clone();

        if file_exists {
            local_present += 1;
        } else if let Some(existing_backup) = att
            .backup_source_path
            .as_deref()
            .filter(|path| Path::new(path).exists())
        {
            backup_source_path = Some(existing_backup.to_string());
        } else if let Some(backup_root) = backup_path {
            let filename = att.filename.as_deref().unwrap_or("");
            if !filename.is_empty() {
                if let Some(found_backup_file) =
                    recovery::ios_backup::scan_for_attachment(backup_root, filename)
                {
                    let found_backup_str = found_backup_file.to_string_lossy().to_string();
                    backup_source_path = Some(found_backup_str.clone());
                    backup_found += 1;

                    if copy {
                        if let Some(resolved) = resolved_path.as_deref() {
                            let dst = PathBuf::from(resolved);
                            match recovery::ios_backup::copy_from_backup(&found_backup_file, &dst) {
                                Ok(_) => {
                                    file_exists = true;
                                    copied += 1;
                                }
                                Err(e) => {
                                    pb.println(format!(
                                        "Failed to copy {} during repair: {e}",
                                        att.display_name()
                                    ));
                                    errors += 1;
                                }
                            }
                        } else {
                            pb.println(format!(
                                "No resolved path available to copy {}",
                                att.display_name()
                            ));
                            errors += 1;
                        }
                    }
                }
            }
        }

        if resolved_path != att.resolved_path
            || file_exists != att.file_exists
            || backup_source_path != att.backup_source_path
        {
            if let Err(e) = db::queries::update_attachment_availability(
                &conn,
                att.id,
                resolved_path.as_deref(),
                file_exists,
                backup_source_path.as_deref(),
            ) {
                pb.println(format!("Failed to update attachment {}: {e}", att.id));
                errors += 1;
            } else {
                updated += 1;
            }
        }

        pb.inc(1);
    }

    pb.finish_with_message("done");
    println!();
    println!("Repair complete:");
    println!("  Total checked:   {}", attachments.len());
    println!("  Updated rows:    {updated}");
    println!("  Local present:   {local_present}");
    if backup_path.is_some() {
        println!("  Found in backup: {backup_found}");
    }
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
        Commands::RepairAttachments { backup_path, copy } => {
            if let Err(err) = repair_attachments(backup_path.as_deref(), copy) {
                eprintln!("repair-attachments failed: {err}");
                std::process::exit(1);
            }
        }
    }
}
