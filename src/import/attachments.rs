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
    ck_sync_state: i64,
    ck_record_id: Option<String>,
    is_sticker: bool,
    hide_attachment: bool,
}

/// Import attachments from the source database.
///
/// `since_message_rowid`: if Some, only import attachments for source messages with
/// ROWID > this value (incremental). If None, import all attachments (full import).
///
/// Returns the number of attachments imported.
pub fn import_attachments(
    source_db: &Path,
    port_db: &mut Connection,
    since_message_rowid: Option<i64>,
) -> Result<u64> {
    let source_conn =
        imessage_database::tables::table::get_connection(source_db).map_err(|_| Error)?;

    let message_id_map = build_message_id_map(port_db)?;

    let home = std::env::var("HOME").unwrap_or_default();

    let base_query = "SELECT a.rowid, a.guid, a.filename, a.mime_type, a.uti, a.transfer_name, a.total_bytes,
                             COALESCE(a.ck_sync_state, 0), a.ck_record_id, a.is_sticker, a.hide_attachment,
                             maj.message_id
                      FROM message_attachment_join maj
                      JOIN attachment a ON maj.attachment_id = a.rowid";

    let base_count = "SELECT COUNT(*) FROM message_attachment_join maj
                      JOIN attachment a ON maj.attachment_id = a.rowid";

    let (query, count_query) = match since_message_rowid {
        Some(hwm) => (
            format!("{base_query} WHERE maj.message_id > {hwm}"),
            format!("{base_count} WHERE maj.message_id > {hwm}"),
        ),
        None => (base_query.to_string(), base_count.to_string()),
    };

    let total: i64 = source_conn
        .query_row(&count_query, [], |row| row.get(0))
        .map_err(|_| Error)?;

    if total == 0 {
        return Ok(0);
    }

    let progress = ProgressBar::new(total as u64);
    progress.set_style(
        ProgressStyle::with_template("{bar:40.cyan/blue} {pos}/{len} attachments")
            .unwrap_or_else(|_| ProgressStyle::default_bar()),
    );

    let mut stmt = source_conn.prepare(&query).map_err(|_| Error)?;

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
                row.get::<_, Option<String>>(8)?,
                row.get::<_, bool>(9)?,
                row.get::<_, bool>(10)?,
                row.get::<_, i64>(11)?,
            ))
        })
        .map_err(|_| Error)?;

    let mut batch: Vec<AttachmentRow> = Vec::with_capacity(5000);
    let mut count: u64 = 0;

    for row_result in rows {
        progress.inc(1);
        let (
            apple_id,
            guid,
            filename,
            mime_type,
            uti,
            transfer_name,
            total_bytes,
            ck_sync_state,
            ck_record_id,
            is_sticker,
            hide_attachment,
            source_msg_id,
        ) = row_result.map_err(|_| Error)?;

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
            ck_sync_state,
            ck_record_id,
            is_sticker,
            hide_attachment,
        });
        count += 1;

        if batch.len() >= 5000 {
            insert_attachment_batch(port_db, &batch)?;
            batch.clear();
        }
    }

    if !batch.is_empty() {
        insert_attachment_batch(port_db, &batch)?;
    }

    progress.finish();
    Ok(count)
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

pub fn resolve_path(filename: Option<&str>, home: &str) -> (Option<String>, bool) {
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
                "INSERT INTO attachments
                    (message_id, apple_attachment_id, guid, filename, resolved_path, mime_type, uti, transfer_name, total_bytes, file_exists, ck_sync_state, ck_record_id, is_sticker, hide_attachment)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                 ON CONFLICT(message_id, apple_attachment_id) DO UPDATE SET
                     guid = excluded.guid,
                     filename = excluded.filename,
                     resolved_path = excluded.resolved_path,
                     mime_type = excluded.mime_type,
                     uti = excluded.uti,
                     transfer_name = excluded.transfer_name,
                     total_bytes = excluded.total_bytes,
                     file_exists = excluded.file_exists,
                     ck_sync_state = excluded.ck_sync_state,
                     ck_record_id = excluded.ck_record_id,
                     is_sticker = excluded.is_sticker,
                     hide_attachment = excluded.hide_attachment",
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
                row.ck_sync_state,
                row.ck_record_id.as_deref(),
                row.is_sticker,
                row.hide_attachment,
            ))
            .map_err(|_| Error)?;
        }
    }

    tx.commit().map_err(|_| Error)?;
    Ok(())
}
