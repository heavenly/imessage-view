use chrono::NaiveDate;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct MessageRow {
    pub id: i64,
    pub guid: String,
    pub body: Option<String>,
    pub is_from_me: bool,
    pub date_unix: i64,
    pub service: Option<String>,
    pub sender_name: Option<String>,
    pub has_attachments: bool,
    pub sender_id: Option<i64>,
    pub has_sender_photo: bool,
}

#[derive(Debug, Serialize)]
pub struct MessageAttachment {
    pub id: i64,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub uti: Option<String>,
    pub transfer_name: Option<String>,
    pub total_bytes: Option<i64>,
    pub is_sticker: bool,
}

impl MessageAttachment {
    pub fn mime_category(&self) -> &'static str {
        infer_attachment_category(
            self.mime_type.as_deref(),
            self.filename.as_deref(),
            self.transfer_name.as_deref(),
            self.uti.as_deref(),
        )
    }
}

#[derive(Debug, Serialize)]
pub struct MessageReaction {
    pub target_guid: String,
    pub is_from_me: bool,
    pub sender_name: Option<String>,
    pub reaction_type: i64,
    pub reaction_emoji: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GroupReactionHighlightRow {
    pub message_id: i64,
    pub message_body: Option<String>,
    pub message_date_unix: i64,
    pub message_sender_name: Option<String>,
    pub message_has_attachments: bool,
    pub reaction_is_from_me: bool,
    pub reaction_sender_name: Option<String>,
    pub reaction_type: i64,
}

fn map_message_row(row: &rusqlite::Row) -> rusqlite::Result<MessageRow> {
    Ok(MessageRow {
        id: row.get(0)?,
        guid: row.get(1)?,
        body: row.get(2)?,
        is_from_me: row.get(3)?,
        date_unix: row.get(4)?,
        service: row.get(5)?,
        sender_name: row.get(6)?,
        has_attachments: row.get(7)?,
        sender_id: row.get(8)?,
        has_sender_photo: row.get::<_, bool>(9).unwrap_or(false),
    })
}

#[derive(Debug, Serialize)]
pub struct ConversationInfo {
    pub id: i64,
    pub display_name: Option<String>,
    pub is_group: bool,
    pub participant_names: Vec<String>,
    pub has_photo: bool,
}

pub fn get_conversation_info(
    conn: &Connection,
    conversation_id: i64,
) -> anyhow::Result<ConversationInfo> {
    let row = conn.query_row(
        "SELECT id, display_name, is_group, group_photo_path FROM conversations WHERE id = ?1",
        [conversation_id],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, bool>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        },
    )?;

    let mut stmt = conn.prepare(
        "SELECT COALESCE(ct.display_name, ct.handle) AS name
         FROM conversation_participants cp
         JOIN contacts ct ON ct.id = cp.contact_id
         WHERE cp.conversation_id = ?1
         ORDER BY name",
    )?;
    let names: Vec<String> = stmt
        .query_map([conversation_id], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;

    let has_photo = if row.2 {
        row.3.is_some()
    } else {
        let photo_exists: bool = conn
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM conversation_participants cp
                    JOIN contacts ct ON ct.id = cp.contact_id
                    WHERE cp.conversation_id = ?1 AND ct.photo IS NOT NULL
                    LIMIT 1
                )",
                [conversation_id],
                |r| r.get(0),
            )
            .unwrap_or(false);
        photo_exists
    };

    Ok(ConversationInfo {
        id: row.0,
        display_name: row.1,
        is_group: row.2,
        participant_names: names,
        has_photo,
    })
}

pub fn get_conversation_first_message_unix(
    conn: &Connection,
    conversation_id: i64,
) -> anyhow::Result<Option<i64>> {
    Ok(conn.query_row(
        "SELECT MIN(date_unix)
         FROM messages
         WHERE conversation_id = ?1
            AND is_reaction = 0",
        [conversation_id],
        |row| row.get(0),
    )?)
}

pub fn get_primary_contact_id_for_conversation(
    conn: &Connection,
    conversation_id: i64,
) -> anyhow::Result<Option<i64>> {
    let contact_id = conn
        .query_row(
            "SELECT cp.contact_id
             FROM conversation_participants cp
             JOIN conversations c ON c.id = cp.conversation_id
             WHERE cp.conversation_id = ?1
               AND c.is_group = 0
             ORDER BY cp.contact_id
             LIMIT 1",
            [conversation_id],
            |row| row.get(0),
        )
        .optional()?;

    Ok(contact_id)
}

pub fn resolve_canonical_conversation_id(
    conn: &Connection,
    conversation_id: i64,
) -> anyhow::Result<Option<i64>> {
    let canonical_id = conn
        .query_row(
            "SELECT canonical_conversation_id
             FROM conversation_aliases
             WHERE source_conversation_id = ?1",
            [conversation_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();

    if canonical_id.is_some() {
        return Ok(canonical_id);
    }

    let existing_id = conn
        .query_row(
            "SELECT id FROM conversations WHERE id = ?1",
            [conversation_id],
            |row| row.get(0),
        )
        .optional()?;

    Ok(existing_id)
}

#[derive(Debug)]
struct MergeConversationRow {
    id: i64,
    is_group: bool,
    display_name: Option<String>,
    service: Option<String>,
    group_photo_path: Option<String>,
    last_message_date: Option<i64>,
}

fn merge_key_for_conversation(
    conn: &Connection,
    conversation_id: i64,
) -> anyhow::Result<Option<String>> {
    let conversation = conn
        .query_row(
            "SELECT is_group FROM conversations WHERE id = ?1",
            [conversation_id],
            |row| row.get::<_, bool>(0),
        )
        .optional()?;

    let Some(is_group) = conversation else {
        return Ok(None);
    };

    let mut stmt = conn.prepare(
        "SELECT contact_id
         FROM conversation_participants
         WHERE conversation_id = ?1
         ORDER BY contact_id",
    )?;
    let participants = stmt
        .query_map([conversation_id], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;

    if participants.is_empty() {
        return Ok(None);
    }

    let key = if is_group {
        participants
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",")
    } else {
        format!("solo:{}", participants[0])
    };

    Ok(Some(key))
}

fn refresh_conversation_rollups(conn: &Connection) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE conversations
         SET participant_count = (
                 SELECT COUNT(*)
                 FROM conversation_participants cp
                 WHERE cp.conversation_id = conversations.id
             ),
             message_count = (
                 SELECT COUNT(*)
                 FROM messages
                 WHERE messages.conversation_id = conversations.id
                   AND messages.is_reaction = FALSE
             ),
             last_message_date = (
                 SELECT MAX(date_unix)
                 FROM messages
                 WHERE messages.conversation_id = conversations.id
             )",
        [],
    )?;

    Ok(())
}

pub fn merge_duplicate_conversations(conn: &Connection) -> anyhow::Result<()> {
    let mut stmt = conn.prepare(
        "SELECT id, is_group, display_name, service, group_photo_path, last_message_date
         FROM conversations
         ORDER BY id",
    )?;
    let conversations = stmt
        .query_map([], |row| {
            Ok(MergeConversationRow {
                id: row.get(0)?,
                is_group: row.get(1)?,
                display_name: row.get(2)?,
                service: row.get(3)?,
                group_photo_path: row.get(4)?,
                last_message_date: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut grouped_ids: HashMap<String, Vec<i64>> = HashMap::new();
    for conversation in &conversations {
        if let Some(key) = merge_key_for_conversation(conn, conversation.id)? {
            grouped_ids.entry(key).or_default().push(conversation.id);
        }
    }

    let conversation_by_id: HashMap<i64, &MergeConversationRow> = conversations
        .iter()
        .map(|conversation| (conversation.id, conversation))
        .collect();

    let tx = conn.unchecked_transaction()?;

    for ids in grouped_ids.values() {
        if ids.len() <= 1 {
            continue;
        }

        let mut canonical_id = *ids.iter().min().unwrap_or(&ids[0]);
        if !conversation_by_id.contains_key(&canonical_id) {
            canonical_id = ids[0];
        }

        let mut latest_named: Option<&MergeConversationRow> = None;
        let mut latest_with_photo: Option<&MergeConversationRow> = None;
        let mut latest_service: Option<&MergeConversationRow> = None;

        for id in ids {
            let Some(conversation) = conversation_by_id.get(id).copied() else {
                continue;
            };

            if conversation
                .display_name
                .as_ref()
                .is_some_and(|name| !name.trim().is_empty())
                && latest_named.is_none_or(|current| {
                    conversation.last_message_date.unwrap_or(0)
                        > current.last_message_date.unwrap_or(0)
                })
            {
                latest_named = Some(conversation);
            }

            if conversation.group_photo_path.is_some()
                && latest_with_photo.is_none_or(|current| {
                    conversation.last_message_date.unwrap_or(0)
                        > current.last_message_date.unwrap_or(0)
                })
            {
                latest_with_photo = Some(conversation);
            }

            if conversation.service.is_some()
                && latest_service.is_none_or(|current| {
                    conversation.last_message_date.unwrap_or(0)
                        > current.last_message_date.unwrap_or(0)
                })
            {
                latest_service = Some(conversation);
            }
        }

        for duplicate_id in ids {
            if *duplicate_id == canonical_id {
                tx.execute(
                    "DELETE FROM conversation_aliases WHERE source_conversation_id = ?1",
                    [*duplicate_id],
                )?;
                continue;
            }

            tx.execute(
                "INSERT INTO conversation_aliases (source_conversation_id, canonical_conversation_id)
                 VALUES (?1, ?2)
                 ON CONFLICT(source_conversation_id)
                 DO UPDATE SET canonical_conversation_id = excluded.canonical_conversation_id",
                rusqlite::params![duplicate_id, canonical_id],
            )?;
            tx.execute(
                "UPDATE messages SET conversation_id = ?1 WHERE conversation_id = ?2",
                rusqlite::params![canonical_id, duplicate_id],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO conversation_participants (conversation_id, contact_id)
                 SELECT ?1, contact_id
                 FROM conversation_participants
                 WHERE conversation_id = ?2",
                rusqlite::params![canonical_id, duplicate_id],
            )?;
            tx.execute(
                "DELETE FROM conversation_participants WHERE conversation_id = ?1",
                [*duplicate_id],
            )?;
            tx.execute("DELETE FROM conversations WHERE id = ?1", [*duplicate_id])?;
        }

        let display_name = latest_named.and_then(|conversation| conversation.display_name.clone());
        let group_photo_path =
            latest_with_photo.and_then(|conversation| conversation.group_photo_path.clone());
        let service = latest_service.and_then(|conversation| conversation.service.clone());
        let is_group = conversation_by_id
            .get(&canonical_id)
            .map(|conversation| conversation.is_group)
            .unwrap_or(false);

        tx.execute(
            "UPDATE conversations
             SET display_name = COALESCE(?2, display_name),
                 service = COALESCE(?3, service),
                 group_photo_path = CASE WHEN ?4 THEN COALESCE(?5, group_photo_path) ELSE group_photo_path END
             WHERE id = ?1",
            rusqlite::params![canonical_id, display_name, service, is_group, group_photo_path],
        )?;
    }

    tx.execute(
        "DELETE FROM conversation_aliases
         WHERE canonical_conversation_id NOT IN (SELECT id FROM conversations)",
        [],
    )?;

    tx.commit()?;
    refresh_conversation_rollups(conn)?;

    Ok(())
}

pub fn get_messages(
    conn: &Connection,
    conversation_id: i64,
    page: u32,
    per_page: u32,
) -> anyhow::Result<Vec<MessageRow>> {
    let offset = page * per_page;
    let mut stmt = conn.prepare(
        "SELECT m.id,
                m.guid,
                m.body,
                m.is_from_me,
                m.date_unix,
                m.service,
                COALESCE(ct.display_name, ct.handle) AS sender_name,
                m.has_attachments,
                ct.id AS sender_id,
                (ct.photo IS NOT NULL) AS has_sender_photo
         FROM messages m
         LEFT JOIN contacts ct ON ct.id = m.sender_id
         WHERE m.conversation_id = ?1
           AND m.is_reaction = FALSE
         ORDER BY m.date_unix DESC
         LIMIT ?2 OFFSET ?3",
    )?;

    let rows = stmt
        .query_map(
            rusqlite::params![conversation_id, per_page, offset],
            map_message_row,
        )?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

pub struct MessagesAroundResult {
    pub messages: Vec<MessageRow>,
    pub has_older: bool,
    pub has_newer: bool,
}

pub fn get_messages_around(
    conn: &Connection,
    conversation_id: i64,
    target_message_id: i64,
    context: u32,
) -> anyhow::Result<MessagesAroundResult> {
    // Step 1: Get the target message's date_unix
    let (target_date, target_id): (i64, i64) = conn.query_row(
        "SELECT date_unix, id FROM messages WHERE id = ?1 AND conversation_id = ?2",
        rusqlite::params![target_message_id, conversation_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    // Step 2: Get `context + 1` messages BEFORE the target (older), ordered DESC
    let before_limit = context + 1;
    let mut stmt_before = conn.prepare(
        "SELECT m.id, m.guid, m.body, m.is_from_me, m.date_unix, m.service,
                COALESCE(ct.display_name, ct.handle) AS sender_name,
                m.has_attachments,
                ct.id AS sender_id,
                (ct.photo IS NOT NULL) AS has_sender_photo
         FROM messages m
         LEFT JOIN contacts ct ON ct.id = m.sender_id
         WHERE m.conversation_id = ?1
           AND m.is_reaction = FALSE
           AND (m.date_unix < ?2 OR (m.date_unix = ?2 AND m.id < ?3))
         ORDER BY m.date_unix DESC, m.id DESC
         LIMIT ?4",
    )?;
    let before_rows: Vec<MessageRow> = stmt_before
        .query_map(
            rusqlite::params![conversation_id, target_date, target_id, before_limit],
            map_message_row,
        )?
        .collect::<Result<Vec<_>, _>>()?;

    let has_older = before_rows.len() > context as usize;
    let before_rows: Vec<MessageRow> = before_rows.into_iter().take(context as usize).collect();

    // Step 3: Get `context + 1` messages AFTER the target (newer), ordered ASC
    let after_limit = context + 1;
    let mut stmt_after = conn.prepare(
        "SELECT m.id, m.guid, m.body, m.is_from_me, m.date_unix, m.service,
                COALESCE(ct.display_name, ct.handle) AS sender_name,
                m.has_attachments,
                ct.id AS sender_id,
                (ct.photo IS NOT NULL) AS has_sender_photo
         FROM messages m
         LEFT JOIN contacts ct ON ct.id = m.sender_id
         WHERE m.conversation_id = ?1
           AND m.is_reaction = FALSE
           AND (m.date_unix > ?2 OR (m.date_unix = ?2 AND m.id > ?3))
         ORDER BY m.date_unix ASC, m.id ASC
         LIMIT ?4",
    )?;
    let after_rows: Vec<MessageRow> = stmt_after
        .query_map(
            rusqlite::params![conversation_id, target_date, target_id, after_limit],
            map_message_row,
        )?
        .collect::<Result<Vec<_>, _>>()?;

    let has_newer = after_rows.len() > context as usize;
    let after_rows: Vec<MessageRow> = after_rows.into_iter().take(context as usize).collect();

    // Step 4: Get the target message itself
    let target_msg: MessageRow = conn.query_row(
        "SELECT m.id, m.guid, m.body, m.is_from_me, m.date_unix, m.service,
                COALESCE(ct.display_name, ct.handle) AS sender_name,
                m.has_attachments,
                ct.id AS sender_id,
                (ct.photo IS NOT NULL) AS has_sender_photo
         FROM messages m
         LEFT JOIN contacts ct ON ct.id = m.sender_id
         WHERE m.id = ?1
           AND m.is_reaction = FALSE",
        [target_message_id],
        map_message_row,
    )?;

    // Step 5: Combine: before (reversed to chronological) + target + after
    let mut messages = Vec::with_capacity(before_rows.len() + 1 + after_rows.len());
    for m in before_rows.into_iter().rev() {
        messages.push(m);
    }
    messages.push(target_msg);
    for m in after_rows {
        messages.push(m);
    }

    Ok(MessagesAroundResult {
        messages,
        has_older,
        has_newer,
    })
}

pub fn get_messages_before(
    conn: &Connection,
    conversation_id: i64,
    before_id: i64,
    limit: u32,
) -> anyhow::Result<Vec<MessageRow>> {
    let (anchor_date, anchor_id): (i64, i64) = conn.query_row(
        "SELECT date_unix, id FROM messages WHERE id = ?1 AND conversation_id = ?2",
        rusqlite::params![before_id, conversation_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    let mut stmt = conn.prepare(
        "SELECT m.id, m.guid, m.body, m.is_from_me, m.date_unix, m.service,
                COALESCE(ct.display_name, ct.handle) AS sender_name,
                m.has_attachments,
                ct.id AS sender_id,
                (ct.photo IS NOT NULL) AS has_sender_photo
         FROM messages m
         LEFT JOIN contacts ct ON ct.id = m.sender_id
         WHERE m.conversation_id = ?1
           AND m.is_reaction = FALSE
           AND (m.date_unix < ?2 OR (m.date_unix = ?2 AND m.id < ?3))
         ORDER BY m.date_unix DESC, m.id DESC
         LIMIT ?4",
    )?;

    let rows: Vec<MessageRow> = stmt
        .query_map(
            rusqlite::params![conversation_id, anchor_date, anchor_id, limit],
            map_message_row,
        )?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

pub fn get_messages_after(
    conn: &Connection,
    conversation_id: i64,
    after_id: i64,
    limit: u32,
) -> anyhow::Result<Vec<MessageRow>> {
    let (anchor_date, anchor_id): (i64, i64) = conn.query_row(
        "SELECT date_unix, id FROM messages WHERE id = ?1 AND conversation_id = ?2",
        rusqlite::params![after_id, conversation_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    let mut stmt = conn.prepare(
        "SELECT m.id, m.guid, m.body, m.is_from_me, m.date_unix, m.service,
                COALESCE(ct.display_name, ct.handle) AS sender_name,
                m.has_attachments,
                ct.id AS sender_id,
                (ct.photo IS NOT NULL) AS has_sender_photo
         FROM messages m
         LEFT JOIN contacts ct ON ct.id = m.sender_id
         WHERE m.conversation_id = ?1
           AND m.is_reaction = FALSE
           AND (m.date_unix > ?2 OR (m.date_unix = ?2 AND m.id > ?3))
         ORDER BY m.date_unix ASC, m.id ASC
         LIMIT ?4",
    )?;

    let rows: Vec<MessageRow> = stmt
        .query_map(
            rusqlite::params![conversation_id, anchor_date, anchor_id, limit],
            map_message_row,
        )?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

pub fn get_group_reaction_highlight_rows(
    conn: &Connection,
    conversation_id: i64,
) -> anyhow::Result<Vec<GroupReactionHighlightRow>> {
    let mut stmt = conn.prepare(
        "WITH reacted_messages AS (
             SELECT DISTINCT associated_message_guid
             FROM messages
             WHERE conversation_id = ?1
               AND is_reaction = TRUE
               AND reaction_type BETWEEN 2000 AND 3007
               AND associated_message_guid IS NOT NULL
         )
         SELECT tm.id,
                tm.body,
                tm.date_unix,
                COALESCE(tct.display_name, tct.handle) AS message_sender_name,
                tm.has_attachments,
                r.is_from_me,
                COALESCE(rct.display_name, rct.handle) AS reaction_sender_name,
                r.reaction_type
         FROM reacted_messages rm
         JOIN messages tm ON tm.guid = rm.associated_message_guid
         JOIN messages r ON r.associated_message_guid = tm.guid
         LEFT JOIN contacts tct ON tct.id = tm.sender_id
         LEFT JOIN contacts rct ON rct.id = r.sender_id
         WHERE tm.conversation_id = ?1
           AND tm.is_reaction = FALSE
           AND r.conversation_id = ?1
           AND r.is_reaction = TRUE
           AND r.reaction_type BETWEEN 2000 AND 3007
         ORDER BY tm.date_unix DESC, tm.id DESC, r.date_unix ASC, r.id ASC",
    )?;

    let rows = stmt
        .query_map([conversation_id], |row| {
            Ok(GroupReactionHighlightRow {
                message_id: row.get(0)?,
                message_body: row.get(1)?,
                message_date_unix: row.get(2)?,
                message_sender_name: row.get(3)?,
                message_has_attachments: row.get(4)?,
                reaction_is_from_me: row.get(5)?,
                reaction_sender_name: row.get(6)?,
                reaction_type: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

pub fn get_message_attachments(
    conn: &Connection,
    message_ids: &[i64],
) -> anyhow::Result<std::collections::HashMap<i64, Vec<MessageAttachment>>> {
    if message_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let placeholders: Vec<String> = message_ids.iter().map(|_| "?".to_string()).collect();
    let sql = format!(
        "SELECT id, message_id, filename, mime_type, uti, transfer_name, total_bytes, is_sticker
         FROM attachments
         WHERE message_id IN ({})
         ORDER BY id",
        placeholders.join(",")
    );

    let mut stmt = conn.prepare(&sql)?;
    let params: Vec<&dyn rusqlite::types::ToSql> = message_ids
        .iter()
        .map(|id| id as &dyn rusqlite::types::ToSql)
        .collect();

    let rows = stmt
        .query_map(params.as_slice(), |row| {
            Ok((
                row.get::<_, i64>(1)?,
                MessageAttachment {
                    id: row.get(0)?,
                    filename: row.get(2)?,
                    mime_type: row.get(3)?,
                    uti: row.get(4)?,
                    transfer_name: row.get(5)?,
                    total_bytes: row.get(6)?,
                    is_sticker: row.get::<_, bool>(7).unwrap_or(false),
                },
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut map: std::collections::HashMap<i64, Vec<MessageAttachment>> =
        std::collections::HashMap::new();
    for (msg_id, att) in rows {
        map.entry(msg_id).or_default().push(att);
    }
    Ok(map)
}

pub fn get_reactions_for_messages(
    conn: &Connection,
    message_guids: &[String],
) -> anyhow::Result<std::collections::HashMap<String, Vec<MessageReaction>>> {
    if message_guids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let placeholders: Vec<String> = message_guids.iter().map(|_| "?".to_string()).collect();
    let sql = format!(
        "SELECT m.associated_message_guid,
                m.is_from_me,
                COALESCE(ct.display_name, ct.handle) AS sender_name,
                m.reaction_type,
                m.reaction_emoji
         FROM messages m
         LEFT JOIN contacts ct ON ct.id = m.sender_id
         WHERE m.is_reaction = TRUE
           AND m.associated_message_guid IN ({})
         ORDER BY m.date_unix ASC, m.id ASC",
        placeholders.join(",")
    );

    let mut stmt = conn.prepare(&sql)?;
    let params: Vec<&dyn rusqlite::types::ToSql> = message_guids
        .iter()
        .map(|guid| guid as &dyn rusqlite::types::ToSql)
        .collect();

    let rows = stmt
        .query_map(params.as_slice(), |row| {
            Ok((
                row.get::<_, String>(0)?,
                MessageReaction {
                    target_guid: row.get(0)?,
                    is_from_me: row.get(1)?,
                    sender_name: row.get(2)?,
                    reaction_type: row.get(3)?,
                    reaction_emoji: row.get(4)?,
                },
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut map: std::collections::HashMap<String, Vec<MessageReaction>> =
        std::collections::HashMap::new();
    for (guid, reaction) in rows {
        map.entry(guid).or_default().push(reaction);
    }
    Ok(map)
}

#[derive(Debug, Serialize)]
pub struct ConversationListRow {
    pub id: i64,
    pub display_name: Option<String>,
    pub handle: Option<String>,
    pub is_group: bool,
    pub last_message_date: Option<i64>,
    pub message_count: i64,
    pub last_message_preview: Option<String>,
    pub has_photo: bool,
}

pub fn conversation_list(
    conn: &Connection,
    filter: Option<&str>,
) -> anyhow::Result<Vec<ConversationListRow>> {
    let base_sql = "SELECT c.id,
                    c.display_name,
                    (SELECT COALESCE(ct.display_name, ct.handle)
                     FROM conversation_participants cp
                     JOIN contacts ct ON ct.id = cp.contact_id
                     WHERE cp.conversation_id = c.id
                     LIMIT 1) AS handle,
                    c.is_group,
                    c.last_message_date,
                    c.message_count,
                     (SELECT SUBSTR(m.body, 1, 80)
                      FROM messages m
                      WHERE m.conversation_id = c.id
                        AND m.is_reaction = FALSE
                      ORDER BY m.date_unix DESC
                      LIMIT 1) AS last_preview,
                    CASE WHEN c.is_group THEN
                        (c.group_photo_path IS NOT NULL)
                    ELSE
                        EXISTS(
                            SELECT 1 FROM conversation_participants cp2
                            JOIN contacts ct2 ON ct2.id = cp2.contact_id
                            WHERE cp2.conversation_id = c.id AND ct2.photo IS NOT NULL
                            LIMIT 1
                        )
                    END AS has_photo
             FROM conversations c";

    let row_mapper = |row: &rusqlite::Row| {
        Ok(ConversationListRow {
            id: row.get(0)?,
            display_name: row.get(1)?,
            handle: row.get(2)?,
            is_group: row.get(3)?,
            last_message_date: row.get(4)?,
            message_count: row.get(5)?,
            last_message_preview: row.get(6)?,
            has_photo: row.get(7)?,
        })
    };

    if let Some(q) = filter {
        let pattern = format!("%{q}%");
        let sql = format!(
            "{base_sql}
             WHERE c.display_name LIKE ?1
                OR EXISTS (
                    SELECT 1 FROM conversation_participants cp
                    JOIN contacts ct ON ct.id = cp.contact_id
                    WHERE cp.conversation_id = c.id
                      AND (ct.display_name LIKE ?1 OR ct.handle LIKE ?1)
                )
             ORDER BY c.last_message_date DESC NULLS LAST"
        );
        let mut stmt = conn.prepare(&sql)?;
        let mapped = stmt.query_map([&pattern], row_mapper)?;
        let rows = mapped.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    } else {
        let sql = format!(
            "{base_sql}
             ORDER BY c.last_message_date DESC NULLS LAST"
        );
        let mut stmt = conn.prepare(&sql)?;
        let mapped = stmt.query_map([], row_mapper)?;
        let rows = mapped.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

#[derive(Debug, Serialize)]
pub struct AttachmentRow {
    pub id: i64,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub uti: Option<String>,
    pub total_bytes: Option<i64>,
    pub resolved_path: Option<String>,
    pub file_exists: bool,
    pub transfer_name: Option<String>,
    pub conversation_name: Option<String>,
    pub message_date: Option<i64>,
    pub conversation_id: Option<i64>,
    pub ck_sync_state: i64,
    pub backup_source_path: Option<String>,
}

impl AttachmentRow {
    pub fn human_size(&self) -> String {
        match self.total_bytes {
            Some(b) if b >= 1_073_741_824 => format!("{:.1} GB", b as f64 / 1_073_741_824.0),
            Some(b) if b >= 1_048_576 => format!("{:.1} MB", b as f64 / 1_048_576.0),
            Some(b) if b >= 1024 => format!("{:.1} KB", b as f64 / 1024.0),
            Some(b) => format!("{b} B"),
            None => "Unknown".to_string(),
        }
    }

    pub fn display_name(&self) -> &str {
        self.transfer_name
            .as_deref()
            .or(self.filename.as_deref())
            .unwrap_or("Unnamed")
    }

    pub fn mime_category(&self) -> &str {
        infer_attachment_category(
            self.mime_type.as_deref(),
            self.filename.as_deref(),
            self.transfer_name.as_deref(),
            self.uti.as_deref(),
        )
    }

    pub fn inferred_content_type(&self) -> &'static str {
        infer_attachment_content_type(
            self.mime_type.as_deref(),
            self.filename.as_deref(),
            self.transfer_name.as_deref(),
            self.uti.as_deref(),
        )
    }

    pub fn date_formatted(&self) -> String {
        match self.message_date {
            Some(ts) => {
                let dt = chrono::DateTime::from_timestamp(ts, 0);
                match dt {
                    Some(d) => d.format("%b %d, %Y").to_string(),
                    None => String::new(),
                }
            }
            None => String::new(),
        }
    }

    pub fn existing_path(&self) -> Option<&str> {
        self.resolved_path
            .as_deref()
            .filter(|path| Path::new(path).exists())
            .or_else(|| {
                self.backup_source_path
                    .as_deref()
                    .filter(|path| Path::new(path).exists())
            })
    }

    pub fn is_available(&self) -> bool {
        self.existing_path().is_some()
    }
}

fn infer_attachment_category(
    mime_type: Option<&str>,
    filename: Option<&str>,
    transfer_name: Option<&str>,
    uti: Option<&str>,
) -> &'static str {
    media_category_from_mime(mime_type)
        .or_else(|| media_category_from_extension(filename, transfer_name))
        .or_else(|| media_category_from_uti(uti))
        .unwrap_or("other")
}

fn infer_attachment_content_type(
    mime_type: Option<&str>,
    filename: Option<&str>,
    transfer_name: Option<&str>,
    uti: Option<&str>,
) -> &'static str {
    match mime_type.and_then(normalized_media_mime_type) {
        Some(mime) => mime,
        None => guessed_content_type_from_extension(filename, transfer_name)
            .or_else(|| guessed_content_type_from_uti(uti))
            .unwrap_or("application/octet-stream"),
    }
}

fn normalized_media_mime_type(mime_type: &str) -> Option<&'static str> {
    let mime = mime_type.trim().to_ascii_lowercase();
    match mime.as_str() {
        m if m.starts_with("image/") => Some(match m {
            "image/jpg" => "image/jpeg",
            "image/heif" => "image/heif",
            "image/heic" => "image/heic",
            "image/heic-sequence" => "image/heic-sequence",
            "image/png" => "image/png",
            "image/gif" => "image/gif",
            "image/webp" => "image/webp",
            "image/tiff" => "image/tiff",
            "image/avif" => "image/avif",
            _ => "image/jpeg",
        }),
        m if m.starts_with("video/") => Some(match m {
            "video/quicktime" => "video/quicktime",
            "video/mp4" => "video/mp4",
            "video/3gpp" => "video/3gpp",
            "video/webm" => "video/webm",
            _ => "video/mp4",
        }),
        m if m.starts_with("audio/") => Some(match m {
            "audio/mpeg" => "audio/mpeg",
            "audio/mp4" => "audio/mp4",
            "audio/aac" => "audio/aac",
            "audio/wav" => "audio/wav",
            "audio/amr" => "audio/amr",
            _ => "audio/mpeg",
        }),
        _ => None,
    }
}

fn media_category_from_mime(mime_type: Option<&str>) -> Option<&'static str> {
    match mime_type?.trim().to_ascii_lowercase() {
        mime if mime.starts_with("image/") => Some("image"),
        mime if mime.starts_with("video/") => Some("video"),
        mime if mime.starts_with("audio/") => Some("audio"),
        _ => None,
    }
}

fn media_category_from_extension(
    filename: Option<&str>,
    transfer_name: Option<&str>,
) -> Option<&'static str> {
    attachment_extension(transfer_name)
        .or_else(|| attachment_extension(filename))
        .and_then(|ext| match ext.as_str() {
            "jpg" | "jpeg" | "png" | "gif" | "webp" | "heic" | "heif" | "tif" | "tiff" | "dng"
            | "avif" => Some("image"),
            "mov" | "mp4" | "m4v" | "3gp" | "avi" | "webm" => Some("video"),
            "m4a" | "mp3" | "wav" | "aac" | "amr" | "caf" => Some("audio"),
            _ => None,
        })
}

fn media_category_from_uti(uti: Option<&str>) -> Option<&'static str> {
    let uti = uti?.trim().to_ascii_lowercase();
    if uti.starts_with("public.image")
        || matches!(
            uti.as_str(),
            "public.heic"
                | "public.heif"
                | "public.jpeg"
                | "public.png"
                | "public.tiff"
                | "public.webp"
                | "com.compuserve.gif"
                | "com.apple.private.auto-loop-gif"
                | "com.apple.raw-image"
        )
    {
        Some("image")
    } else if uti.starts_with("public.movie") || uti.starts_with("public.video") {
        Some("video")
    } else if uti.starts_with("public.audio") {
        Some("audio")
    } else {
        None
    }
}

fn guessed_content_type_from_extension(
    filename: Option<&str>,
    transfer_name: Option<&str>,
) -> Option<&'static str> {
    attachment_extension(transfer_name)
        .or_else(|| attachment_extension(filename))
        .and_then(|ext| match ext.as_str() {
            "jpg" | "jpeg" => Some("image/jpeg"),
            "png" => Some("image/png"),
            "gif" => Some("image/gif"),
            "webp" => Some("image/webp"),
            "heic" => Some("image/heic"),
            "heif" => Some("image/heif"),
            "tif" | "tiff" => Some("image/tiff"),
            "dng" => Some("image/x-adobe-dng"),
            "avif" => Some("image/avif"),
            "mov" => Some("video/quicktime"),
            "mp4" | "m4v" => Some("video/mp4"),
            "3gp" => Some("video/3gpp"),
            "webm" => Some("video/webm"),
            "m4a" => Some("audio/mp4"),
            "mp3" => Some("audio/mpeg"),
            "wav" => Some("audio/wav"),
            "aac" => Some("audio/aac"),
            "amr" => Some("audio/amr"),
            "caf" => Some("audio/x-caf"),
            _ => None,
        })
}

fn guessed_content_type_from_uti(uti: Option<&str>) -> Option<&'static str> {
    let uti = uti?.trim().to_ascii_lowercase();
    if uti.starts_with("public.image")
        || matches!(uti.as_str(), "public.heic" | "public.heif" | "public.jpeg")
    {
        Some("image/jpeg")
    } else if uti.starts_with("public.movie") || uti.starts_with("public.video") {
        Some("video/mp4")
    } else if uti.starts_with("public.audio") {
        Some("audio/mpeg")
    } else {
        None
    }
}

fn attachment_extension(name: Option<&str>) -> Option<String> {
    Path::new(name?)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
}

fn attachment_category_sql(category: &str) -> String {
    let image = attachment_category_predicate(
        "image",
        &[
            "jpg", "jpeg", "png", "gif", "webp", "heic", "heif", "tif", "tiff", "dng", "avif",
        ],
        &["public.image"],
        &[
            "public.heic",
            "public.heif",
            "public.jpeg",
            "public.png",
            "public.tiff",
            "public.webp",
            "com.compuserve.gif",
            "com.apple.private.auto-loop-gif",
            "com.apple.raw-image",
        ],
    );
    let video = attachment_category_predicate(
        "video",
        &["mov", "mp4", "m4v", "3gp", "avi", "webm"],
        &["public.movie", "public.video"],
        &[],
    );
    let audio = attachment_category_predicate(
        "audio",
        &["m4a", "mp3", "wav", "aac", "amr", "caf"],
        &["public.audio"],
        &[],
    );

    match category {
        "image" => image,
        "video" => video,
        "audio" => audio,
        "other" => format!("NOT (({image}) OR ({video}) OR ({audio}))"),
        _ => "1 = 1".to_string(),
    }
}

fn attachment_category_predicate(
    mime_prefix: &str,
    extensions: &[&str],
    uti_prefixes: &[&str],
    uti_exact: &[&str],
) -> String {
    let mime_predicate = format!("LOWER(COALESCE(a.mime_type, '')) LIKE '{mime_prefix}/%'");
    let extension_predicate = attachment_extension_predicate(extensions);
    let uti_predicate = attachment_uti_predicate(uti_prefixes, uti_exact);
    format!("({mime_predicate} OR {extension_predicate} OR {uti_predicate})")
}

fn attachment_extension_predicate(extensions: &[&str]) -> String {
    let clauses: Vec<String> = extensions
        .iter()
        .map(|ext| {
            format!(
                "LOWER(COALESCE(a.transfer_name, '')) LIKE '%.{ext}' OR LOWER(COALESCE(a.filename, '')) LIKE '%.{ext}'"
            )
        })
        .collect();
    format!("({})", clauses.join(" OR "))
}

fn attachment_uti_predicate(uti_prefixes: &[&str], uti_exact: &[&str]) -> String {
    let mut clauses: Vec<String> = uti_prefixes
        .iter()
        .map(|prefix| format!("LOWER(COALESCE(a.uti, '')) LIKE '{prefix}%'"))
        .collect();
    clauses.extend(
        uti_exact
            .iter()
            .map(|value| format!("LOWER(COALESCE(a.uti, '')) = '{value}'")),
    );
    format!("({})", clauses.join(" OR "))
}

pub fn list_attachments(
    conn: &Connection,
    mime_filter: Option<&str>,
    offset: i64,
    limit: i64,
) -> anyhow::Result<Vec<AttachmentRow>> {
    // Base where clause to exclude pluginPayloadAttachment files
    let base_where = "WHERE (a.filename IS NULL OR a.filename NOT LIKE '%.pluginPayloadAttachment') AND (a.transfer_name IS NULL OR a.transfer_name NOT LIKE '%.pluginPayloadAttachment')";

    let (where_clause, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match mime_filter {
        Some("image") => (
            format!("{} AND {}", base_where, attachment_category_sql("image")),
            vec![],
        ),
        Some("video") => (
            format!("{} AND {}", base_where, attachment_category_sql("video")),
            vec![],
        ),
        Some("audio") => (
            format!("{} AND {}", base_where, attachment_category_sql("audio")),
            vec![],
        ),
        Some("other") => (
            format!("{} AND {}", base_where, attachment_category_sql("other")),
            vec![],
        ),
        _ => (base_where.to_string(), vec![]),
    };

    let sql = format!(
        "SELECT a.id, a.filename, a.mime_type, a.uti, a.total_bytes, a.resolved_path,
                a.file_exists, a.transfer_name,
                COALESCE(c.display_name, c.guid, 'Unknown') AS conversation_name,
                m.date_unix,
                c.id AS conversation_id,
                a.ck_sync_state,
                a.backup_source_path
         FROM attachments a
         JOIN messages m ON m.id = a.message_id
         JOIN conversations c ON c.id = m.conversation_id
         {where_clause}
         ORDER BY m.date_unix DESC, m.id DESC, a.id DESC
         LIMIT {limit} OFFSET {offset}"
    );

    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(AttachmentRow {
                id: row.get(0)?,
                filename: row.get(1)?,
                mime_type: row.get(2)?,
                uti: row.get(3)?,
                total_bytes: row.get(4)?,
                resolved_path: row.get(5)?,
                file_exists: row.get(6)?,
                transfer_name: row.get(7)?,
                conversation_name: row.get(8)?,
                message_date: row.get(9)?,
                conversation_id: row.get(10)?,
                ck_sync_state: row.get(11)?,
                backup_source_path: row.get(12)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

pub fn get_attachment(conn: &Connection, id: i64) -> anyhow::Result<Option<AttachmentRow>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.filename, a.mime_type, a.uti, a.total_bytes, a.resolved_path,
                a.file_exists, a.transfer_name,
                COALESCE(c.display_name, c.guid, 'Unknown') AS conversation_name,
                m.date_unix,
                c.id AS conversation_id,
                a.ck_sync_state,
                a.backup_source_path
         FROM attachments a
         JOIN messages m ON m.id = a.message_id
         JOIN conversations c ON c.id = m.conversation_id
         WHERE a.id = ?1",
    )?;

    let row = stmt
        .query_row([id], |row| {
            Ok(AttachmentRow {
                id: row.get(0)?,
                filename: row.get(1)?,
                mime_type: row.get(2)?,
                uti: row.get(3)?,
                total_bytes: row.get(4)?,
                resolved_path: row.get(5)?,
                file_exists: row.get(6)?,
                transfer_name: row.get(7)?,
                conversation_name: row.get(8)?,
                message_date: row.get(9)?,
                conversation_id: row.get(10)?,
                ck_sync_state: row.get(11)?,
                backup_source_path: row.get(12)?,
            })
        })
        .optional()?;

    Ok(row)
}

pub fn count_attachments(conn: &Connection, mime_filter: Option<&str>) -> anyhow::Result<i64> {
    let base_where = "(filename IS NULL OR filename NOT LIKE '%.pluginPayloadAttachment') AND (transfer_name IS NULL OR transfer_name NOT LIKE '%.pluginPayloadAttachment')";
    let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match mime_filter {
        Some("image") => (
            format!(
                "SELECT COUNT(*) FROM attachments a WHERE {} AND {}",
                base_where,
                attachment_category_sql("image")
            ),
            vec![],
        ),
        Some("video") => (
            format!(
                "SELECT COUNT(*) FROM attachments a WHERE {} AND {}",
                base_where,
                attachment_category_sql("video")
            ),
            vec![],
        ),
        Some("audio") => (
            format!(
                "SELECT COUNT(*) FROM attachments a WHERE {} AND {}",
                base_where,
                attachment_category_sql("audio")
            ),
            vec![],
        ),
        Some("other") => (
            format!(
                "SELECT COUNT(*) FROM attachments a WHERE {} AND {}",
                base_where,
                attachment_category_sql("other")
            ),
            vec![],
        ),
        _ => (
            format!("SELECT COUNT(*) FROM attachments a WHERE {}", base_where),
            vec![],
        ),
    };

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let count: i64 = conn.query_row(&sql, param_refs.as_slice(), |r| r.get(0))?;
    Ok(count)
}

pub fn conversation_attachments(
    conn: &Connection,
    conversation_id: i64,
    offset: i64,
    limit: i64,
) -> anyhow::Result<Vec<AttachmentRow>> {
    let sql = format!(
        "SELECT a.id, a.filename, a.mime_type, a.uti, a.total_bytes, a.resolved_path,
                a.file_exists, a.transfer_name,
                COALESCE(c.display_name, c.guid, 'Unknown') AS conversation_name,
                m.date_unix,
                c.id AS conversation_id,
                a.ck_sync_state,
                a.backup_source_path
         FROM attachments a
         JOIN messages m ON m.id = a.message_id
         JOIN conversations c ON c.id = m.conversation_id
         WHERE m.conversation_id = ?1
           AND (a.filename IS NULL OR a.filename NOT LIKE '%.pluginPayloadAttachment')
           AND (a.transfer_name IS NULL OR a.transfer_name NOT LIKE '%.pluginPayloadAttachment')
         ORDER BY m.date_unix DESC, m.id DESC, a.id DESC
         LIMIT {limit} OFFSET {offset}"
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([conversation_id], |row| {
            Ok(AttachmentRow {
                id: row.get(0)?,
                filename: row.get(1)?,
                mime_type: row.get(2)?,
                uti: row.get(3)?,
                total_bytes: row.get(4)?,
                resolved_path: row.get(5)?,
                file_exists: row.get(6)?,
                transfer_name: row.get(7)?,
                conversation_name: row.get(8)?,
                message_date: row.get(9)?,
                conversation_id: row.get(10)?,
                ck_sync_state: row.get(11)?,
                backup_source_path: row.get(12)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

pub fn count_conversation_attachments(
    conn: &Connection,
    conversation_id: i64,
) -> anyhow::Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*)
         FROM attachments a
         JOIN messages m ON m.id = a.message_id
         WHERE m.conversation_id = ?1
           AND (a.filename IS NULL OR a.filename NOT LIKE '%.pluginPayloadAttachment')
           AND (a.transfer_name IS NULL OR a.transfer_name NOT LIKE '%.pluginPayloadAttachment')",
        [conversation_id],
        |r| r.get(0),
    )?;
    Ok(count)
}

pub fn count_missing_attachments(conn: &Connection) -> anyhow::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM attachments 
         WHERE file_exists = 0
           AND (filename IS NULL OR filename NOT LIKE '%.pluginPayloadAttachment')
           AND (transfer_name IS NULL OR transfer_name NOT LIKE '%.pluginPayloadAttachment')",
        [],
        |r| r.get(0),
    )
    .map_err(Into::into)
}

pub fn count_missing_icloud_attachments(conn: &Connection) -> anyhow::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM attachments 
         WHERE file_exists = 0 AND ck_sync_state = 1
           AND (filename IS NULL OR filename NOT LIKE '%.pluginPayloadAttachment')
           AND (transfer_name IS NULL OR transfer_name NOT LIKE '%.pluginPayloadAttachment')",
        [],
        |r| r.get(0),
    )
    .map_err(Into::into)
}

pub fn count_missing_with_backup(conn: &Connection) -> anyhow::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM attachments 
         WHERE file_exists = 0 AND backup_source_path IS NOT NULL
           AND (filename IS NULL OR filename NOT LIKE '%.pluginPayloadAttachment')
           AND (transfer_name IS NULL OR transfer_name NOT LIKE '%.pluginPayloadAttachment')",
        [],
        |r| r.get(0),
    )
    .map_err(Into::into)
}

pub fn get_missing_attachments(
    conn: &Connection,
    offset: i64,
    limit: i64,
) -> anyhow::Result<Vec<AttachmentRow>> {
    let sql = format!(
        "SELECT a.id, a.filename, a.mime_type, a.uti, a.total_bytes, a.resolved_path,
                a.file_exists, a.transfer_name,
                COALESCE(c.display_name, c.guid, 'Unknown') AS conversation_name,
                m.date_unix,
                c.id AS conversation_id,
                a.ck_sync_state,
                a.backup_source_path
         FROM attachments a
         JOIN messages m ON m.id = a.message_id
         JOIN conversations c ON c.id = m.conversation_id
         WHERE a.file_exists = 0
           AND (a.filename IS NULL OR a.filename NOT LIKE '%.pluginPayloadAttachment')
           AND (a.transfer_name IS NULL OR a.transfer_name NOT LIKE '%.pluginPayloadAttachment')
         ORDER BY m.date_unix ASC
         LIMIT {limit} OFFSET {offset}"
    );

    let mut stmt = conn.prepare(&sql)?;

    let rows = stmt
        .query_map([], |row| {
            Ok(AttachmentRow {
                id: row.get(0)?,
                filename: row.get(1)?,
                mime_type: row.get(2)?,
                uti: row.get(3)?,
                total_bytes: row.get(4)?,
                resolved_path: row.get(5)?,
                file_exists: row.get(6)?,
                transfer_name: row.get(7)?,
                conversation_name: row.get(8)?,
                message_date: row.get(9)?,
                conversation_id: row.get(10)?,
                ck_sync_state: row.get(11)?,
                backup_source_path: row.get(12)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

pub fn all_attachments_for_repair(conn: &Connection) -> anyhow::Result<Vec<AttachmentRow>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.filename, a.mime_type, a.uti, a.total_bytes, a.resolved_path,
                a.file_exists, a.transfer_name,
                COALESCE(c.display_name, c.guid, 'Unknown') AS conversation_name,
                m.date_unix,
                c.id AS conversation_id,
                a.ck_sync_state,
                a.backup_source_path
         FROM attachments a
         JOIN messages m ON m.id = a.message_id
         JOIN conversations c ON c.id = m.conversation_id
         WHERE (a.filename IS NULL OR a.filename NOT LIKE '%.pluginPayloadAttachment')
           AND (a.transfer_name IS NULL OR a.transfer_name NOT LIKE '%.pluginPayloadAttachment')",
    )?;

    let rows = stmt
        .query_map([], |row| {
            Ok(AttachmentRow {
                id: row.get(0)?,
                filename: row.get(1)?,
                mime_type: row.get(2)?,
                uti: row.get(3)?,
                total_bytes: row.get(4)?,
                resolved_path: row.get(5)?,
                file_exists: row.get(6)?,
                transfer_name: row.get(7)?,
                conversation_name: row.get(8)?,
                message_date: row.get(9)?,
                conversation_id: row.get(10)?,
                ck_sync_state: row.get(11)?,
                backup_source_path: row.get(12)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

pub fn update_attachment_availability(
    conn: &Connection,
    id: i64,
    resolved_path: Option<&str>,
    file_exists: bool,
    backup_source_path: Option<&str>,
) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE attachments
         SET resolved_path = ?1,
             file_exists = ?2,
             backup_source_path = ?3
         WHERE id = ?4",
        rusqlite::params![resolved_path, file_exists, backup_source_path, id],
    )?;
    Ok(())
}

pub fn update_attachment_backup_source(
    conn: &Connection,
    id: i64,
    backup_path: &str,
) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE attachments SET backup_source_path = ?1 WHERE id = ?2",
        rusqlite::params![backup_path, id],
    )?;
    Ok(())
}

pub enum ConversationPhoto {
    ContactBlob(Vec<u8>),
    GroupFilePath(String),
}

#[derive(Debug, Serialize, Clone)]
pub struct MutualInteractionDay {
    pub date: String,
    pub sent: i64,
    pub received: i64,
}

pub fn get_mutual_interaction_days(
    conn: &Connection,
    conversation_id: i64,
    days: u32,
) -> anyhow::Result<Vec<MutualInteractionDay>> {
    let mut stmt = conn.prepare(
        "SELECT strftime('%Y-%m-%d', date_unix, 'unixepoch') AS day,
                SUM(CASE WHEN is_from_me = 1 THEN 1 ELSE 0 END) AS sent,
                SUM(CASE WHEN is_from_me = 0 THEN 1 ELSE 0 END) AS received
         FROM messages
         WHERE conversation_id = ?1
           AND date_unix >= CAST(strftime('%s', 'now', ?2) AS INTEGER)
         GROUP BY day
         ORDER BY day",
    )?;

    let offset_param = format!("-{days} days");
    let rows = stmt
        .query_map(rusqlite::params![conversation_id, offset_param], |row| {
            Ok(MutualInteractionDay {
                date: row.get(0)?,
                sent: row.get(1)?,
                received: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

pub struct AvgResponseTimes {
    pub avg_their_response: Option<f64>,
    pub avg_my_response: Option<f64>,
}

/// For 1-1 conversations: compute average response times in both directions.
/// Uses LAG window function to find consecutive message pairs where the sender changes.
/// Excludes gaps > 172800 seconds (48 hours).
pub fn get_avg_response_times(
    conn: &Connection,
    conversation_id: i64,
) -> anyhow::Result<AvgResponseTimes> {
    let sql = "
        WITH ordered AS (
            SELECT date_unix, is_from_me,
                   LAG(date_unix) OVER (ORDER BY date_unix, id) AS prev_date,
                   LAG(is_from_me) OVER (ORDER BY date_unix, id) AS prev_from_me
            FROM messages
            WHERE conversation_id = ?1
        ),
        gaps AS (
            SELECT is_from_me, prev_from_me, (date_unix - prev_date) AS gap
            FROM ordered
            WHERE prev_date IS NOT NULL
              AND is_from_me != prev_from_me
              AND (date_unix - prev_date) > 0
              AND (date_unix - prev_date) <= 172800
        )
        SELECT
            AVG(CASE WHEN is_from_me = 0 AND prev_from_me = 1 THEN gap END),
            AVG(CASE WHEN is_from_me = 1 AND prev_from_me = 0 THEN gap END)
        FROM gaps
    ";

    let result = conn.query_row(sql, [conversation_id], |row| {
        Ok(AvgResponseTimes {
            avg_their_response: row.get(0)?,
            avg_my_response: row.get(1)?,
        })
    })?;

    Ok(result)
}

/// For group conversations: compute average time between consecutive messages.
/// Excludes gaps > 172800 seconds (48 hours).
pub fn get_avg_time_between_messages(
    conn: &Connection,
    conversation_id: i64,
) -> anyhow::Result<Option<f64>> {
    let sql = "
        WITH ordered AS (
            SELECT date_unix,
                   LAG(date_unix) OVER (ORDER BY date_unix, id) AS prev_date
            FROM messages
            WHERE conversation_id = ?1
        )
        SELECT AVG(date_unix - prev_date)
        FROM ordered
        WHERE prev_date IS NOT NULL
          AND (date_unix - prev_date) > 0
          AND (date_unix - prev_date) <= 172800
    ";

    let result: Option<f64> = conn.query_row(sql, [conversation_id], |row| row.get(0))?;
    Ok(result)
}

pub fn get_conversation_photo(
    conn: &Connection,
    conversation_id: i64,
) -> anyhow::Result<Option<ConversationPhoto>> {
    let row = conn.query_row(
        "SELECT is_group, group_photo_path FROM conversations WHERE id = ?1",
        [conversation_id],
        |row| Ok((row.get::<_, bool>(0)?, row.get::<_, Option<String>>(1)?)),
    )?;

    let (is_group, group_photo_path) = row;

    if is_group {
        Ok(group_photo_path.map(ConversationPhoto::GroupFilePath))
    } else {
        let photo: Option<Vec<u8>> = conn
            .query_row(
                "SELECT ct.photo FROM conversation_participants cp
                 JOIN contacts ct ON ct.id = cp.contact_id
                 WHERE cp.conversation_id = ?1 AND ct.photo IS NOT NULL
                 LIMIT 1",
                [conversation_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();

        Ok(photo.map(ConversationPhoto::ContactBlob))
    }
}

pub fn get_contact_photo(conn: &Connection, contact_id: i64) -> anyhow::Result<Option<Vec<u8>>> {
    use rusqlite::OptionalExtension;
    let photo: Option<Vec<u8>> = conn
        .query_row(
            "SELECT photo FROM contacts WHERE id = ?1 AND photo IS NOT NULL",
            [contact_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    Ok(photo)
}

#[derive(Debug, Serialize)]
pub struct GroupParticipantStat {
    pub contact_id: Option<i64>,
    pub name: String,
    pub message_count: i64,
    pub percentage: String,
    pub has_photo: bool,
}

pub fn get_group_participant_stats(
    conn: &Connection,
    conversation_id: i64,
) -> anyhow::Result<Vec<GroupParticipantStat>> {
    let mut stmt = conn.prepare(
        "WITH agg AS (
             SELECT sender_id,
                    is_from_me,
                    COUNT(*) AS cnt
             FROM messages
             WHERE conversation_id = ?1
             GROUP BY is_from_me, sender_id
         )
         SELECT agg.sender_id,
                agg.is_from_me,
                COALESCE(ct.display_name, ct.handle, 'Unknown') AS name,
                agg.cnt,
                (ct.photo IS NOT NULL) AS has_photo
         FROM agg
         LEFT JOIN contacts ct ON ct.id = agg.sender_id
         ORDER BY agg.cnt DESC",
    )?;

    let rows: Vec<(Option<i64>, bool, String, i64, bool)> = stmt
        .query_map([conversation_id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get::<_, bool>(4).unwrap_or(false),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let total: i64 = rows.iter().map(|r| r.3).sum();
    let total_f = if total > 0 { total as f64 } else { 1.0 };

    let stats = rows
        .into_iter()
        .map(
            |(sender_id, is_from_me, name, count, has_photo)| GroupParticipantStat {
                contact_id: if is_from_me { None } else { sender_id },
                name: if is_from_me { "Me".to_string() } else { name },
                message_count: count,
                percentage: format!("{:.1}", (count as f64 / total_f) * 100.0),
                has_photo: if is_from_me { false } else { has_photo },
            },
        )
        .collect();

    Ok(stats)
}

#[derive(Debug, Serialize)]
pub struct HourlyStat {
    pub hour: u8,
    pub count: i64,
    pub pct: f64,
}

pub fn get_hourly_message_stats(
    conn: &Connection,
    conversation_id: i64,
) -> anyhow::Result<Vec<HourlyStat>> {
    let mut stmt = conn.prepare(
        "SELECT CAST(strftime('%H', date_unix, 'unixepoch', 'localtime') AS INTEGER) AS hour,
                COUNT(*) AS cnt
         FROM messages
         WHERE conversation_id = ?1
         GROUP BY hour
         ORDER BY hour",
    )?;

    let rows: Vec<(u8, i64)> = stmt
        .query_map([conversation_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;

    let mut counts = [0i64; 24];
    for (h, c) in &rows {
        counts[*h as usize] = *c;
    }
    let max_count = counts.iter().copied().max().unwrap_or(1).max(1) as f64;

    let stats = (0..24u8)
        .map(|h| HourlyStat {
            hour: h,
            count: counts[h as usize],
            pct: (counts[h as usize] as f64 / max_count) * 100.0,
        })
        .collect();

    Ok(stats)
}

#[derive(Debug, Serialize)]
pub struct ContactBasicInfo {
    pub id: i64,
    pub name: String,
    pub handle: String,
    pub has_photo: bool,
}

pub fn get_contact_basic_info(
    conn: &Connection,
    contact_id: i64,
) -> anyhow::Result<Option<ContactBasicInfo>> {
    let info = conn
        .query_row(
            "SELECT id,
                    COALESCE(display_name, handle, 'Unknown') AS name,
                    handle,
                    (photo IS NOT NULL) AS has_photo
             FROM contacts
             WHERE id = ?1",
            [contact_id],
            |row| {
                Ok(ContactBasicInfo {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    handle: row.get(2)?,
                    has_photo: row.get::<_, bool>(3).unwrap_or(false),
                })
            },
        )
        .optional()?;

    Ok(info)
}

pub fn get_contact_conversation_id(
    conn: &Connection,
    contact_id: i64,
) -> anyhow::Result<Option<i64>> {
    let conversation_id = conn
        .query_row(
            "SELECT c.id
             FROM conversation_participants cp
             JOIN conversations c ON c.id = cp.conversation_id
             WHERE cp.contact_id = ?1
               AND c.is_group = 0
             ORDER BY c.last_message_date DESC, c.id DESC
             LIMIT 1",
            [contact_id],
            |row| row.get(0),
        )
        .optional()?;

    Ok(conversation_id)
}

#[derive(Debug, Serialize, Default)]
pub struct ContactMessageCounts {
    pub sent: i64,
    pub received: i64,
}

pub fn get_contact_message_counts(
    conn: &Connection,
    conversation_id: i64,
) -> anyhow::Result<ContactMessageCounts> {
    let counts = conn.query_row(
        "SELECT COALESCE(SUM(CASE WHEN is_from_me = 1 THEN 1 ELSE 0 END), 0) AS sent,
                COALESCE(SUM(CASE WHEN is_from_me = 0 THEN 1 ELSE 0 END), 0) AS received
         FROM messages
         WHERE conversation_id = ?1
           AND is_reaction = 0",
        [conversation_id],
        |row| {
            Ok(ContactMessageCounts {
                sent: row.get(0)?,
                received: row.get(1)?,
            })
        },
    )?;

    Ok(counts)
}

#[derive(Debug, Serialize, Default)]
pub struct ContactDateRange {
    pub first_message: Option<String>,
    pub last_message: Option<String>,
}

pub fn get_contact_first_last_dates(
    conn: &Connection,
    conversation_id: i64,
) -> anyhow::Result<ContactDateRange> {
    let range = conn.query_row(
        "SELECT strftime('%Y-%m-%d', MIN(date_unix), 'unixepoch', 'localtime'),
                strftime('%Y-%m-%d', MAX(date_unix), 'unixepoch', 'localtime')
         FROM messages
         WHERE conversation_id = ?1
           AND is_reaction = 0",
        [conversation_id],
        |row| {
            Ok(ContactDateRange {
                first_message: row.get(0)?,
                last_message: row.get(1)?,
            })
        },
    )?;

    Ok(range)
}

pub fn get_contact_longest_streak(conn: &Connection, conversation_id: i64) -> anyhow::Result<i64> {
    let mut stmt = conn.prepare(
        "SELECT day
         FROM (
             SELECT strftime('%Y-%m-%d', date_unix, 'unixepoch', 'localtime') AS day,
                    SUM(CASE WHEN is_from_me = 1 THEN 1 ELSE 0 END) AS sent,
                    SUM(CASE WHEN is_from_me = 0 THEN 1 ELSE 0 END) AS received
             FROM messages
             WHERE conversation_id = ?1
               AND is_reaction = 0
             GROUP BY day
         )
         WHERE sent > 0 AND received > 0
         ORDER BY day",
    )?;

    let days = stmt
        .query_map([conversation_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;

    let mut longest = 0i64;
    let mut current = 0i64;
    let mut previous_day: Option<NaiveDate> = None;

    for day in days {
        let parsed_day = match NaiveDate::parse_from_str(&day, "%Y-%m-%d") {
            Ok(day) => day,
            Err(_) => continue,
        };

        current = if let Some(previous) = previous_day {
            if parsed_day.signed_duration_since(previous).num_days() == 1 {
                current + 1
            } else {
                1
            }
        } else {
            1
        };

        longest = longest.max(current);
        previous_day = Some(parsed_day);
    }

    Ok(longest)
}

#[derive(Debug, Serialize, Default)]
pub struct ContactInitiativeStats {
    pub my_starts: i64,
    pub their_starts: i64,
}

pub fn get_contact_initiative_stats(
    conn: &Connection,
    conversation_id: i64,
) -> anyhow::Result<ContactInitiativeStats> {
    let stats = conn.query_row(
        "WITH ordered AS (
             SELECT id,
                    date_unix,
                    is_from_me,
                    LAG(date_unix) OVER (ORDER BY date_unix, id) AS prev_date
             FROM messages
             WHERE conversation_id = ?1
               AND is_reaction = 0
         ),
         starters AS (
             SELECT is_from_me
             FROM ordered
             WHERE prev_date IS NULL OR (date_unix - prev_date) > 14400
         )
         SELECT COALESCE(SUM(CASE WHEN is_from_me = 1 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN is_from_me = 0 THEN 1 ELSE 0 END), 0)
         FROM starters",
        [conversation_id],
        |row| {
            Ok(ContactInitiativeStats {
                my_starts: row.get(0)?,
                their_starts: row.get(1)?,
            })
        },
    )?;

    Ok(stats)
}

#[derive(Debug, Serialize, Clone)]
pub struct DayOfWeekStat {
    pub day: u8,
    pub label: String,
    pub count: i64,
    pub pct: f64,
}

pub fn get_contact_day_of_week_stats(
    conn: &Connection,
    conversation_id: i64,
) -> anyhow::Result<Vec<DayOfWeekStat>> {
    let mut stmt = conn.prepare(
        "SELECT CAST(strftime('%w', date_unix, 'unixepoch', 'localtime') AS INTEGER) AS weekday,
                COUNT(*) AS cnt
         FROM messages
         WHERE conversation_id = ?1
           AND is_reaction = 0
         GROUP BY weekday",
    )?;

    let rows = stmt
        .query_map([conversation_id], |row| {
            Ok((row.get::<_, u8>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut counts = [0i64; 7];
    for (weekday, count) in rows {
        counts[weekday as usize] = count;
    }

    let ordered_days = [
        (1u8, "Mon"),
        (2u8, "Tue"),
        (3u8, "Wed"),
        (4u8, "Thu"),
        (5u8, "Fri"),
        (6u8, "Sat"),
        (0u8, "Sun"),
    ];
    let max_count = ordered_days
        .iter()
        .map(|(weekday, _)| counts[*weekday as usize])
        .max()
        .unwrap_or(0)
        .max(1) as f64;

    let stats = ordered_days
        .into_iter()
        .enumerate()
        .map(|(index, (weekday, label))| DayOfWeekStat {
            day: index as u8,
            label: label.to_string(),
            count: counts[weekday as usize],
            pct: (counts[weekday as usize] as f64 / max_count) * 100.0,
        })
        .collect();

    Ok(stats)
}

#[derive(Debug, Serialize, Default)]
pub struct ContactReactionCounts {
    pub my_reactions: i64,
    pub their_reactions: i64,
}

pub fn get_contact_reaction_counts(
    conn: &Connection,
    conversation_id: i64,
) -> anyhow::Result<ContactReactionCounts> {
    let counts = conn.query_row(
        "SELECT COALESCE(SUM(CASE WHEN is_from_me = 1 AND is_reaction = 1 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN is_from_me = 0 AND is_reaction = 1 THEN 1 ELSE 0 END), 0)
         FROM messages
         WHERE conversation_id = ?1",
        [conversation_id],
        |row| {
            Ok(ContactReactionCounts {
                my_reactions: row.get(0)?,
                their_reactions: row.get(1)?,
            })
        },
    )?;

    Ok(counts)
}

#[derive(Debug, Serialize, Default)]
pub struct ContactTrendStats {
    pub recent_count: i64,
    pub prior_count: i64,
}

pub fn get_contact_trend_stats(
    conn: &Connection,
    conversation_id: i64,
) -> anyhow::Result<ContactTrendStats> {
    let stats = conn.query_row(
        "SELECT
             COALESCE(SUM(CASE
                 WHEN date_unix >= CAST(strftime('%s', 'now', '-90 days') AS INTEGER) THEN 1
                 ELSE 0
             END), 0) AS recent_count,
             COALESCE(SUM(CASE
                 WHEN date_unix >= CAST(strftime('%s', 'now', '-180 days') AS INTEGER)
                  AND date_unix < CAST(strftime('%s', 'now', '-90 days') AS INTEGER) THEN 1
                 ELSE 0
             END), 0) AS prior_count
         FROM messages
         WHERE conversation_id = ?1
           AND is_reaction = 0",
        [conversation_id],
        |row| {
            Ok(ContactTrendStats {
                recent_count: row.get(0)?,
                prior_count: row.get(1)?,
            })
        },
    )?;

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::db::schema;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        schema::create_all_tables(&conn).unwrap();
        conn
    }

    #[test]
    fn test_merge_duplicate_conversations_group_threads() {
        let conn = test_conn();

        conn.execute(
            "INSERT INTO contacts (id, handle, display_name) VALUES (1, '+15550000001', 'A')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO contacts (id, handle, display_name) VALUES (2, '+15550000002', 'B')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO conversations (id, apple_chat_id, guid, display_name, is_group, service, last_message_date, message_count, participant_count)
             VALUES (3, 3, 'chat3', 'Older Name', 1, 'iMessage', 100, 1, 2)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO conversations (id, apple_chat_id, guid, display_name, is_group, service, last_message_date, message_count, participant_count)
             VALUES (1160, 1160, 'chat1160', 'Latest Name', 1, 'SMS', 200, 1, 2)",
            [],
        )
        .unwrap();

        for conversation_id in [3_i64, 1160_i64] {
            conn.execute(
                "INSERT INTO conversation_participants (conversation_id, contact_id) VALUES (?1, 1)",
                [conversation_id],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO conversation_participants (conversation_id, contact_id) VALUES (?1, 2)",
                [conversation_id],
            )
            .unwrap();
        }

        conn.execute(
            "INSERT INTO messages (id, apple_message_id, guid, conversation_id, is_from_me, date_unix, is_reaction)
             VALUES (1, 1, 'm1', 3, 0, 100, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (id, apple_message_id, guid, conversation_id, is_from_me, date_unix, is_reaction)
             VALUES (2, 2, 'm2', 1160, 0, 200, 0)",
            [],
        )
        .unwrap();

        merge_duplicate_conversations(&conn).unwrap();

        let remaining_ids: Vec<i64> = conn
            .prepare("SELECT id FROM conversations ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(remaining_ids, vec![3]);

        let alias_target: i64 = conn
            .query_row(
                "SELECT canonical_conversation_id FROM conversation_aliases WHERE source_conversation_id = 1160",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(alias_target, 3);

        let merged_message_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE conversation_id = 3 AND is_reaction = 0",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(merged_message_count, 2);

        let latest_name: String = conn
            .query_row(
                "SELECT display_name FROM conversations WHERE id = 3",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(latest_name, "Latest Name");
    }
}
