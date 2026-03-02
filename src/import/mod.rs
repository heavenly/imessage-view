use std::fs;
use std::path::PathBuf;

use crate::db;

pub mod attachments;
pub mod contacts;
pub mod messages;

pub fn run_import() -> anyhow::Result<()> {
    eprintln!("DEBUG: Starting import...");
    
    let home = std::env::var("HOME")?;
    let source_path = PathBuf::from(home).join("Library/Messages/chat.db");
    eprintln!("DEBUG: Source path: {:?}", source_path);
    
    let data_dir = PathBuf::from("data");
    fs::create_dir_all(&data_dir)?;
    let source_copy = data_dir.join("source_chat.db");
    eprintln!("DEBUG: Copying to: {:?}", source_copy);
    
    fs::copy(&source_path, &source_copy)?;
    eprintln!("DEBUG: Copy completed");

    let port_path = data_dir.join("imessage.db");
    eprintln!("DEBUG: Creating port database at: {:?}", port_path);
    let mut port_db = db::drop_and_recreate(&port_path)?;
    eprintln!("DEBUG: Port database created");
    
    let contacts_map = contacts::resolve_contacts();
    eprintln!("DEBUG: Resolved {} contacts", contacts_map.len());

    match messages::import_messages(&source_copy, &mut port_db, contacts_map) {
        Ok(_) => println!("Message import completed successfully"),
        Err(e) => {
            eprintln!("Message import failed with error: {:?}", e);
            return Err(anyhow::anyhow!("message import failed"));
        }
    }

    match attachments::import_attachments(&source_copy, &mut port_db) {
        Ok(_) => println!("Attachment import completed successfully"),
        Err(e) => {
            eprintln!("Attachment import failed with error: {:?}", e);
            return Err(anyhow::anyhow!("attachment import failed"));
        }
    }

    Ok(())
}
