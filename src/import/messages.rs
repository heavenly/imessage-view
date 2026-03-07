use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

use imessage_database::{
    tables::{
        chat::Chat,
        chat_handle::ChatToHandle,
        handle::Handle,
        messages::Message,
        table::{get_connection, Cacheable, Table},
    },
    util::query_context::QueryContext,
};
use indicatif::{ProgressBar, ProgressStyle};
use rusqlite::Connection;

use crate::error::{Error, Result};
use crate::import::contacts::ContactInfo;

struct MessageRow {
    apple_message_id: i64,
    guid: String,
    conversation_id: i64,
    sender_id: Option<i64>,
    is_from_me: bool,
    body: Option<String>,
    date_unix: i64,
    service: Option<String>,
    is_reaction: bool,
    reaction_type: Option<i64>,
    thread_originator_guid: Option<String>,
    is_edited: bool,
    has_attachments: bool,
    balloon_bundle_id: Option<String>,
}

/// Import messages from the source database.
///
/// `since_rowid`: if Some, only import messages with source ROWID > this value (incremental).
/// If None, import all messages (full import).
///
/// Returns the number of messages imported.
pub fn import_messages(
    source_db: &Path,
    port_db: &mut Connection,
    contacts_map: HashMap<String, ContactInfo>,
    since_rowid: Option<i64>,
) -> Result<u64> {
    let source_conn = get_connection(source_db).map_err(|_| Error)?;

    eprintln!("Importing contacts...");
    let (handle_ids, handle_id_map) = import_contacts(&source_conn, port_db, &contacts_map)?;
    eprintln!("Imported {} unique contacts", handle_ids.len());

    eprintln!("Caching chat participants...");
    let participant_map = ChatToHandle::cache(&source_conn).map_err(|_| Error)?;
    eprintln!("Cached {} chat participants", participant_map.len());

    eprintln!("Importing conversations...");
    import_conversations(port_db, &participant_map, &handle_id_map, &source_conn)?;
    eprintln!("Importing group photos...");
    import_group_photos(port_db, &source_conn)?;
    eprintln!("Importing conversation participants...");
    import_conversation_participants(port_db, &participant_map, &handle_ids, &handle_id_map)?;

    let count = match since_rowid {
        Some(hwm) => import_messages_incremental(&source_conn, port_db, &handle_id_map, hwm)?,
        None => import_messages_full(&source_conn, port_db, &handle_id_map)?,
    };

    port_db
        .execute(
            "INSERT INTO messages_fts(messages_fts) VALUES('rebuild')",
            [],
        )
        .map_err(|_| Error)?;

    port_db
        .execute(
            "UPDATE conversations
             SET message_count = (
                 SELECT COUNT(*) FROM messages WHERE messages.conversation_id = conversations.id
             ),
             last_message_date = (
                 SELECT MAX(date_unix) FROM messages WHERE messages.conversation_id = conversations.id
             )",
            [],
        )
        .map_err(|_| Error)?;

    Ok(count)
}

fn import_messages_full(
    source_conn: &Connection,
    port_db: &mut Connection,
    handle_id_map: &HashMap<i64, i64>,
) -> Result<u64> {
    let context = QueryContext::default();
    let total = Message::get_count(source_conn, &context).map_err(|_| Error)?;
    eprintln!("Importing {} messages...", total);
    let progress = ProgressBar::new(total.max(0) as u64);
    progress.set_style(
        ProgressStyle::with_template("{bar:40.cyan/blue} {pos}/{len} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_bar()),
    );

    let mut statement = Message::stream_rows(source_conn, &context).map_err(|_| Error)?;
    let rows = statement
        .query_map([], |row| Ok(Message::from_row(row)))
        .map_err(|_| Error)?;

    let mut batch: Vec<MessageRow> = Vec::with_capacity(5000);
    let mut count: u64 = 0;
    for message_result in rows {
        let mut message = Message::extract(message_result).map_err(|_| Error)?;
        progress.inc(1);

        if let Some(msg_row) = process_message(&mut message, source_conn, handle_id_map) {
            batch.push(msg_row);
            count += 1;
        }

        if batch.len() >= 5000 {
            insert_message_batch(port_db, &batch)?;
            batch.clear();
        }
    }

    if !batch.is_empty() {
        insert_message_batch(port_db, &batch)?;
    }

    progress.finish();
    Ok(count)
}

fn import_messages_incremental(
    source_conn: &Connection,
    port_db: &mut Connection,
    handle_id_map: &HashMap<i64, i64>,
    since_rowid: i64,
) -> Result<u64> {
    let total: i64 = source_conn
        .query_row(
            "SELECT COUNT(*) FROM message WHERE ROWID > ?1",
            [since_rowid],
            |row| row.get(0),
        )
        .map_err(|_| Error)?;

    if total == 0 {
        return Ok(0);
    }

    eprintln!("Found {} new messages (ROWID > {})...", total, since_rowid);
    let progress = ProgressBar::new(total as u64);
    progress.set_style(
        ProgressStyle::with_template("{bar:40.cyan/blue} {pos}/{len} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_bar()),
    );

    let context = QueryContext::default();
    let mut statement = Message::stream_rows(source_conn, &context).map_err(|_| Error)?;
    let rows = statement
        .query_map([], |row| Ok(Message::from_row(row)))
        .map_err(|_| Error)?;

    let mut batch: Vec<MessageRow> = Vec::with_capacity(5000);
    let mut count: u64 = 0;
    for message_result in rows {
        let mut message = Message::extract(message_result).map_err(|_| Error)?;

        if (message.rowid as i64) <= since_rowid {
            continue;
        }

        progress.inc(1);

        if let Some(msg_row) = process_message(&mut message, source_conn, handle_id_map) {
            batch.push(msg_row);
            count += 1;
        }

        if batch.len() >= 5000 {
            insert_message_batch(port_db, &batch)?;
            batch.clear();
        }
    }

    if !batch.is_empty() {
        insert_message_batch(port_db, &batch)?;
    }

    progress.finish();
    Ok(count)
}

fn process_message(
    message: &mut Message,
    source_conn: &Connection,
    handle_id_map: &HashMap<i64, i64>,
) -> Option<MessageRow> {
    if let Some(assoc_type) = message.associated_message_type {
        if (1000..=4000).contains(&assoc_type) {
            return None;
        }
    }

    let decoded = match message.generate_text(source_conn) {
        Ok(text) => Some(strip_apple_replacements(text)),
        Err(_) => message.text.as_deref().map(strip_apple_replacements),
    };

    let chat_id = message.chat_id? as i64;

    let sender_id = message
        .handle_id
        .map(|id| id as i64)
        .and_then(|id| handle_id_map.get(&id).copied());

    let date_unix = message.date / 1_000_000_000 + 978_307_200;
    let is_from_me = message.is_from_me();

    Some(MessageRow {
        apple_message_id: message.rowid as i64,
        guid: message.guid.clone(),
        conversation_id: chat_id,
        sender_id,
        is_from_me,
        body: decoded,
        date_unix,
        service: message.service.clone(),
        is_reaction: false,
        reaction_type: None,
        thread_originator_guid: message.thread_originator_guid.clone(),
        is_edited: message.is_edited(),
        has_attachments: message.has_attachments(),
        balloon_bundle_id: message.balloon_bundle_id.clone(),
    })
}

fn import_contacts(
    source_conn: &Connection,
    port_db: &mut Connection,
    contacts_map: &HashMap<String, ContactInfo>,
) -> Result<(HashSet<i64>, HashMap<i64, i64>)> {
    let mut ids: HashSet<i64> = HashSet::new();
    let mut handle_id_map: HashMap<i64, i64> = HashMap::new();
    let mut handle_to_canonical: HashMap<String, i64> = HashMap::new();
    let tx = port_db.transaction().map_err(|_| Error)?;
    {
        let mut stmt = tx
            .prepare_cached(
                "INSERT INTO contacts (id, handle, display_name, service, person_centric_id, photo)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(handle) DO UPDATE SET
                     display_name = excluded.display_name,
                     service = excluded.service,
                     person_centric_id = excluded.person_centric_id,
                     photo = COALESCE(excluded.photo, contacts.photo)",
            )
            .map_err(|_| Error)?;

        Handle::stream(source_conn, |result| {
            let handle = result.map_err(|_| Error)?;
            let handle_id = handle.rowid as i64;

            let contact_info = lookup_contact_info(&handle.id, contacts_map);
            let display_name = contact_info.as_ref().map(|ci| ci.display_name.as_str());
            let photo = contact_info.as_ref().and_then(|ci| ci.photo.as_deref());

            let _rows_affected = stmt
                .execute((
                    handle_id,
                    &handle.id,
                    display_name,
                    None::<&str>,
                    handle.person_centric_id.as_deref(),
                    photo,
                ))
                .map_err(|_e| Error)?;

            if let Some(&canonical_id) = handle_to_canonical.get(&handle.id) {
                handle_id_map.insert(handle_id, canonical_id);
            } else {
                ids.insert(handle_id);
                handle_to_canonical.insert(handle.id.clone(), handle_id);
                handle_id_map.insert(handle_id, handle_id);
            }

            Ok::<(), Error>(())
        })
        .map_err(|_| Error)?;
    }

    tx.commit().map_err(|_| Error)?;
    Ok((ids, handle_id_map))
}

fn import_conversations(
    port_db: &mut Connection,
    participant_map: &HashMap<i32, BTreeSet<i32>>,
    handle_id_map: &HashMap<i64, i64>,
    source_conn: &Connection,
) -> Result<()> {
    let tx = port_db.transaction().map_err(|_| Error)?;
    {
        let mut stmt = tx
            .prepare_cached(
                "INSERT INTO conversations
                    (id, apple_chat_id, guid, display_name, is_group, service, last_message_date, message_count, participant_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, 0, ?7)
                 ON CONFLICT(guid) DO UPDATE SET
                     display_name = excluded.display_name,
                     service = excluded.service,
                     participant_count = excluded.participant_count",
            )
            .map_err(|_| Error)?;

        Chat::stream(source_conn, |result| {
            let chat = result.map_err(|_| Error)?;
            let chat_id = chat.rowid as i64;
            let participants = participant_map.get(&chat.rowid);
            let participant_count = participants
                .map(|set| {
                    set.iter()
                        .filter_map(|id| {
                            let handle_id = *id as i64;
                            handle_id_map.get(&handle_id).map(|_| ())
                        })
                        .count() as i64
                })
                .unwrap_or(0);
            let is_group = participant_count > 1;

            stmt.execute((
                chat_id,
                chat_id,
                chat.chat_identifier.as_str(),
                chat.display_name.as_deref(),
                is_group,
                chat.service_name.as_deref(),
                participant_count,
            ))
            .map_err(|_| Error)?;
            Ok::<(), Error>(())
        })
        .map_err(|_| Error)?;
    }

    tx.commit().map_err(|_| Error)?;
    Ok(())
}

fn import_conversation_participants(
    port_db: &mut Connection,
    participant_map: &HashMap<i32, BTreeSet<i32>>,
    _handle_ids: &HashSet<i64>,
    handle_id_map: &HashMap<i64, i64>,
) -> Result<()> {
    let tx = port_db.transaction().map_err(|_| Error)?;
    {
        let mut stmt = tx
            .prepare_cached(
                "INSERT OR IGNORE INTO conversation_participants (conversation_id, contact_id)
                 VALUES (?1, ?2)",
            )
            .map_err(|_| Error)?;

        let mut _count = 0;
        for (chat_id, handles) in participant_map {
            let conversation_id = *chat_id as i64;
            for handle_id in handles {
                let source_handle_id = *handle_id as i64;
                let contact_id = match handle_id_map.get(&source_handle_id) {
                    Some(&canonical_id) => canonical_id,
                    None => {
                        continue;
                    }
                };
                stmt.execute((conversation_id, contact_id))
                    .map_err(|_| Error)?;
                _count += 1;
            }
        }
    }

    tx.commit().map_err(|_| Error)?;
    Ok(())
}

fn import_group_photos(port_db: &mut Connection, source_conn: &Connection) -> Result<()> {
    let mut stmt = source_conn
        .prepare("SELECT ROWID, properties FROM chat WHERE properties IS NOT NULL")
        .map_err(|_| Error)?;

    let rows = stmt
        .query_map([], |row| {
            let rowid: i64 = row.get(0)?;
            let props: Option<Vec<u8>> = row.get(1)?;
            Ok((rowid, props))
        })
        .map_err(|_| Error)?;

    let mut update_stmt = port_db
        .prepare_cached("UPDATE conversations SET group_photo_path = ?1 WHERE id = ?2")
        .map_err(|_| Error)?;

    let home = std::env::var("HOME").unwrap_or_default();

    for row in rows.flatten() {
        let (chat_rowid, props_blob) = row;
        let props_blob = match props_blob {
            Some(b) if !b.is_empty() => b,
            _ => continue,
        };

        let group_photo_guid = match extract_group_photo_guid(&props_blob) {
            Some(guid) => guid,
            None => continue,
        };

        let attachment_path = match resolve_group_photo_path(source_conn, &group_photo_guid, &home)
        {
            Some(p) => p,
            None => continue,
        };

        update_stmt
            .execute(rusqlite::params![attachment_path, chat_rowid])
            .map_err(|_| Error)?;
    }

    Ok(())
}

fn extract_group_photo_guid(props_blob: &[u8]) -> Option<String> {
    let value: plist::Value = plist::from_bytes(props_blob).ok()?;
    let dict = value.as_dictionary()?;
    let guid = dict.get("groupPhotoGuid")?.as_string()?;
    Some(guid.to_string())
}

fn resolve_group_photo_path(source_conn: &Connection, guid: &str, home: &str) -> Option<String> {
    let prefixed_guid = if guid.starts_with("at_") {
        guid.to_string()
    } else {
        format!("at_0_{guid}")
    };

    let mut stmt = source_conn
        .prepare("SELECT filename FROM attachment WHERE guid = ?1")
        .ok()?;

    let filename: Option<String> = stmt.query_row([&prefixed_guid], |row| row.get(0)).ok()?;

    let filename = filename?;
    let resolved = if filename.starts_with('~') {
        filename.replacen('~', home, 1)
    } else {
        filename
    };

    if std::path::Path::new(&resolved).exists() {
        Some(resolved)
    } else {
        None
    }
}

fn insert_message_batch(port_db: &mut Connection, batch: &[MessageRow]) -> Result<()> {
    let tx = port_db.transaction().map_err(|_| Error)?;
    {
        let mut stmt = tx
            .prepare_cached(
                "INSERT OR REPLACE INTO messages
                    (apple_message_id, guid, conversation_id, sender_id, is_from_me, body, date_unix, service, is_reaction, reaction_type, thread_originator_guid, is_edited, has_attachments, balloon_bundle_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            )
            .map_err(|_| Error)?;

        for row in batch {
            stmt.execute((
                row.apple_message_id,
                row.guid.as_str(),
                row.conversation_id,
                row.sender_id,
                row.is_from_me,
                row.body.as_deref(),
                row.date_unix,
                row.service.as_deref(),
                row.is_reaction,
                row.reaction_type,
                row.thread_originator_guid.as_deref(),
                row.is_edited,
                row.has_attachments,
                row.balloon_bundle_id.as_deref(),
            ))
            .map_err(|_| Error)?;
        }
    }

    tx.commit().map_err(|_| Error)?;
    Ok(())
}

fn lookup_contact_info<'a>(
    handle_id: &str,
    contacts_map: &'a HashMap<String, ContactInfo>,
) -> Option<&'a ContactInfo> {
    if let Some(info) = contacts_map.get(handle_id) {
        return Some(info);
    }

    let normalized = if handle_id.contains('@') {
        handle_id.trim().to_lowercase()
    } else {
        let digits: String = handle_id.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.len() == 11 && digits.starts_with('1') {
            digits[1..].to_string()
        } else {
            digits
        }
    };

    contacts_map.get(&normalized)
}

fn strip_apple_replacements(text: &str) -> String {
    text.chars()
        .filter(|c| *c != '\u{FFFC}' && *c != '\u{FFFD}')
        .collect()
}
