use std::collections::HashMap;
use std::path::Path;

use indicatif::{ProgressBar, ProgressStyle};
use rusqlite::Connection;

use crate::error::{Error, Result};

struct AttachmentRow {
    apple_attachment_id: i64,
    message_id: i64,
    guid: Option<String>,
    filename: Option<String>,
    resolved_path: Option<String>,
    mime_type: Option<String>,
    uti: Option<String>,
    transfer_name: Option<String>,
    total_bytes: Option<i64>,
    file_exists: bool,
}

pub fn import_attachments(source_db: &Path, port_db: &mut Connection) -> Result<()> {
    let source_conn =
        imessage_database::tables::table::get_connection(source_db).map_err(|_| Error)?;

    let message_id_map = build_message_id_map(port_db)?;

    let home = std::env::var("HOME").unwrap_or_default();

    let total = count_attachments(&source_conn)?;
    let progress = ProgressBar::new(total as u64);
    progress.set_style(
        ProgressStyle::with_template("{bar:40.cyan/blue} {pos}/{len} attachments")
            .unwrap_or_else(|_| ProgressStyle::default_bar()),
    );

    let mut stmt = source_conn
        .prepare(
            "SELECT a.rowid, a.guid, a.filename, a.mime_type, a.uti, a.transfer_name, a.total_bytes,
                    maj.message_id
             FROM message_attachment_join maj
             JOIN attachment a ON maj.attachment_id = a.rowid",
        )
        .map_err(|_| Error)?;

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })
        .map_err(|_| Error)?;

    let mut batch: Vec<AttachmentRow> = Vec::with_capacity(5000);

    for row_result in rows {
        progress.inc(1);
        let (apple_id, guid, filename, mime_type, uti, transfer_name, total_bytes, source_msg_id) =
            row_result.map_err(|_| Error)?;

        let message_id = match message_id_map.get(&source_msg_id) {
            Some(&id) => id,
            None => continue,
        };

        let (resolved_path, file_exists) = resolve_path(filename.as_deref(), &home);

        batch.push(AttachmentRow {
            apple_attachment_id: apple_id,
            message_id,
            guid,
            filename,
            resolved_path,
            mime_type,
            uti,
            transfer_name,
            total_bytes,
            file_exists,
        });

        if batch.len() >= 5000 {
            insert_attachment_batch(port_db, &batch)?;
            batch.clear();
        }
    }

    if !batch.is_empty() {
        insert_attachment_batch(port_db, &batch)?;
    }

    progress.finish();
    Ok(())
}

fn build_message_id_map(port_db: &Connection) -> Result<HashMap<i64, i64>> {
    let mut stmt = port_db
        .prepare("SELECT apple_message_id, id FROM messages WHERE apple_message_id IS NOT NULL")
        .map_err(|_| Error)?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
        .map_err(|_| Error)?;

    let mut map = HashMap::new();
    for row in rows {
        let (apple_id, port_id) = row.map_err(|_| Error)?;
        map.insert(apple_id, port_id);
    }
    Ok(map)
}

fn count_attachments(source_conn: &Connection) -> Result<i64> {
    source_conn
        .query_row(
            "SELECT COUNT(*) FROM message_attachment_join maj
             JOIN attachment a ON maj.attachment_id = a.rowid",
            [],
            |row| row.get(0),
        )
        .map_err(|_| Error)
}

fn resolve_path(filename: Option<&str>, home: &str) -> (Option<String>, bool) {
    let path_str = match filename {
        Some(f) if !f.is_empty() => f,
        _ => return (None, false),
    };

    let resolved = if path_str.starts_with('~') {
        path_str.replacen('~', home, 1)
    } else {
        path_str.to_string()
    };

    let exists = Path::new(&resolved).exists();
    (Some(resolved), exists)
}

fn insert_attachment_batch(port_db: &mut Connection, batch: &[AttachmentRow]) -> Result<()> {
    let tx = port_db.transaction().map_err(|_| Error)?;
    {
        let mut stmt = tx
            .prepare_cached(
                "INSERT OR REPLACE INTO attachments
                    (message_id, apple_attachment_id, guid, filename, resolved_path, mime_type, uti, transfer_name, total_bytes, file_exists)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )
            .map_err(|_| Error)?;

        for row in batch {
            stmt.execute((
                row.message_id,
                row.apple_attachment_id,
                row.guid.as_deref(),
                row.filename.as_deref(),
                row.resolved_path.as_deref(),
                row.mime_type.as_deref(),
                row.uti.as_deref(),
                row.transfer_name.as_deref(),
                row.total_bytes,
                row.file_exists,
            ))
            .map_err(|_| Error)?;
        }
    }

    tx.commit().map_err(|_| Error)?;
    Ok(())
}
