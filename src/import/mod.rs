use std::fs;
use std::path::PathBuf;

use crate::db;

pub mod attachments;
pub mod contacts;
pub mod messages;

pub fn run_import(force_full: bool) -> anyhow::Result<()> {
    let home = std::env::var("HOME")?;
    let source_path = PathBuf::from(&home).join("Library/Messages/chat.db");
    eprintln!("Source: {:?}", source_path);

    let data_dir = PathBuf::from("data");
    fs::create_dir_all(&data_dir)?;
    let source_copy = data_dir.join("source_chat.db");

    eprintln!("Copying source database...");
    fs::copy(&source_path, &source_copy)?;

    let port_path = data_dir.join("imessage.db");

    let needs_full = if force_full {
        eprintln!("Full reimport requested via --full flag.");
        true
    } else if !port_path.exists() {
        eprintln!("No existing database found. Running full import.");
        true
    } else {
        let conn = db::open_existing(&port_path)?;
        if db::has_current_schema(&conn) {
            false
        } else {
            eprintln!(
                "Schema outdated (missing attachment unique constraint). Running full import."
            );
            true
        }
    };

    if needs_full {
        run_full_import(&source_copy, &port_path)
    } else {
        run_incremental_import(&source_copy, &port_path)
    }
}

fn run_full_import(source_copy: &PathBuf, port_path: &PathBuf) -> anyhow::Result<()> {
    eprintln!("=== Full Import ===");
    let mut port_db = db::drop_and_recreate(port_path)?;

    let contacts_map = contacts::resolve_contacts();
    eprintln!("Resolved {} contacts", contacts_map.len());

    match messages::import_messages(source_copy, &mut port_db, contacts_map, None) {
        Ok(count) => eprintln!("Imported {count} messages"),
        Err(e) => {
            eprintln!("Message import failed: {:?}", e);
            return Err(anyhow::anyhow!("message import failed"));
        }
    }

    match attachments::import_attachments(source_copy, &mut port_db, None) {
        Ok(count) => eprintln!("Imported {count} attachments"),
        Err(e) => {
            eprintln!("Attachment import failed: {:?}", e);
            return Err(anyhow::anyhow!("attachment import failed"));
        }
    }

    db::queries::merge_duplicate_conversations(&port_db)?;

    eprintln!("Full import completed successfully.");
    Ok(())
}

fn run_incremental_import(source_copy: &PathBuf, port_path: &PathBuf) -> anyhow::Result<()> {
    let mut port_db = db::open_existing(port_path)?;
    let high_water = db::get_high_water_mark(&port_db);
    eprintln!("=== Incremental Import (messages after ROWID {high_water}) ===");

    let contacts_map = contacts::resolve_contacts();
    eprintln!("Resolved {} contacts", contacts_map.len());

    match messages::import_messages(source_copy, &mut port_db, contacts_map, Some(high_water)) {
        Ok(count) => {
            if count == 0 {
                eprintln!("No new messages found.");
            } else {
                eprintln!("Imported {count} new messages");
            }
        }
        Err(e) => {
            eprintln!("Message import failed: {:?}", e);
            return Err(anyhow::anyhow!("message import failed"));
        }
    }

    match attachments::import_attachments(source_copy, &mut port_db, Some(high_water)) {
        Ok(count) => {
            if count == 0 {
                eprintln!("No new attachments found.");
            } else {
                eprintln!("Imported {count} new attachments");
            }
        }
        Err(e) => {
            eprintln!("Attachment import failed: {:?}", e);
            return Err(anyhow::anyhow!("attachment import failed"));
        }
    }

    db::queries::merge_duplicate_conversations(&port_db)?;

    eprintln!("Incremental import completed successfully.");
    Ok(())
}
