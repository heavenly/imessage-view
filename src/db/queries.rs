use chrono::NaiveDate;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct AttachmentStats {
    pub total: i64,
    pub images: i64,
    pub videos: i64,
    pub audio: i64,
    pub other: i64,
    pub total_bytes: i64,
}

#[derive(Debug, Serialize)]
pub struct OverallStats {
    pub total_messages: i64,
    pub total_conversations: i64,
    pub total_contacts: i64,
    pub total_attachments: i64,
    pub earliest_message: Option<String>,
    pub latest_message: Option<String>,
}

pub fn messages_per_conversation(
    conn: &Connection,
    limit: u32,
) -> anyhow::Result<Vec<(i64, String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT c.id,
                COALESCE(c.display_name, c.guid, 'Unknown') AS name,
                COUNT(m.id) AS cnt
         FROM conversations c
         JOIN messages m ON m.conversation_id = c.id
         GROUP BY c.id
         ORDER BY cnt DESC
         LIMIT ?1",
    )?;

    let rows = stmt
        .query_map([limit], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

pub fn messages_over_time(
    conn: &Connection,
    granularity: &str,
) -> anyhow::Result<Vec<(String, i64)>> {
    let fmt = match granularity {
        "day" => "%Y-%m-%d",
        "week" => "%Y-W%W",
        "month" => "%Y-%m",
        _ => "%Y-%m-%d",
    };

    let sql = format!(
        "SELECT strftime('{fmt}', date_unix, 'unixepoch') AS period,
                COUNT(*) AS cnt
         FROM messages
         WHERE date_unix IS NOT NULL
         GROUP BY period
         ORDER BY period"
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

pub fn top_contacts(
    conn: &Connection,
    limit: u32,
) -> anyhow::Result<Vec<(i64, String, String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT ct.id,
                COALESCE(ct.display_name, ct.handle) AS name,
                ct.handle,
                COUNT(m.id) AS cnt
         FROM contacts ct
         JOIN messages m ON m.sender_id = ct.id
         GROUP BY ct.id
         ORDER BY cnt DESC
         LIMIT ?1",
    )?;

    let rows = stmt
        .query_map([limit], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

pub fn attachment_stats(conn: &Connection) -> anyhow::Result<AttachmentStats> {
    let stats = conn.query_row(
        "SELECT COUNT(*) AS total,
                SUM(CASE WHEN mime_type LIKE 'image/%' THEN 1 ELSE 0 END) AS images,
                SUM(CASE WHEN mime_type LIKE 'video/%' THEN 1 ELSE 0 END) AS videos,
                SUM(CASE WHEN mime_type LIKE 'audio/%' THEN 1 ELSE 0 END) AS audio,
                SUM(CASE WHEN mime_type NOT LIKE 'image/%'
                          AND mime_type NOT LIKE 'video/%'
                          AND mime_type NOT LIKE 'audio/%' THEN 1 ELSE 0 END) AS other,
                COALESCE(SUM(total_bytes), 0) AS total_bytes
         FROM attachments
         WHERE (filename IS NULL OR filename NOT LIKE '%.pluginPayloadAttachment')
           AND (transfer_name IS NULL OR transfer_name NOT LIKE '%.pluginPayloadAttachment')",
        [],
        |row| {
            Ok(AttachmentStats {
                total: row.get(0)?,
                images: row.get(1)?,
                videos: row.get(2)?,
                audio: row.get(3)?,
                other: row.get(4)?,
                total_bytes: row.get(5)?,
            })
        },
    )?;

    Ok(stats)
}

pub fn overall_stats(conn: &Connection) -> anyhow::Result<OverallStats> {
    let total_messages: i64 = conn.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))?;
    let total_conversations: i64 =
        conn.query_row("SELECT COUNT(*) FROM conversations", [], |r| r.get(0))?;
    let total_contacts: i64 = conn.query_row("SELECT COUNT(*) FROM contacts", [], |r| r.get(0))?;
    let total_attachments: i64 =
        conn.query_row("SELECT COUNT(*) FROM attachments", [], |r| r.get(0))?;

    let earliest_message: Option<String> = conn
        .query_row(
            "SELECT strftime('%Y-%m-%d', MIN(date_unix), 'unixepoch') FROM messages",
            [],
            |r| r.get(0),
        )
        .ok();

    let latest_message: Option<String> = conn
        .query_row(
            "SELECT strftime('%Y-%m-%d', MAX(date_unix), 'unixepoch') FROM messages",
            [],
            |r| r.get(0),
        )
        .ok();

    Ok(OverallStats {
        total_messages,
        total_conversations,
        total_contacts,
        total_attachments,
        earliest_message,
        latest_message,
    })
}

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
    pub transfer_name: Option<String>,
    pub total_bytes: Option<i64>,
    pub is_sticker: bool,
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
pub struct ConversationReactionMessage {
    pub id: i64,
    pub guid: String,
    pub body: Option<String>,
    pub date_unix: i64,
    pub sender_name: Option<String>,
    pub has_attachments: bool,
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

pub fn get_conversation_reaction_messages(
    conn: &Connection,
    conversation_id: i64,
) -> anyhow::Result<Vec<ConversationReactionMessage>> {
    let mut stmt = conn.prepare(
        "SELECT m.id,
                m.guid,
                m.body,
                m.date_unix,
                COALESCE(ct.display_name, ct.handle) AS sender_name,
                m.has_attachments
         FROM messages m
         LEFT JOIN contacts ct ON ct.id = m.sender_id
         WHERE m.conversation_id = ?1
           AND m.is_reaction = FALSE
         ORDER BY m.date_unix DESC, m.id DESC",
    )?;

    let rows = stmt
        .query_map([conversation_id], |row| {
            Ok(ConversationReactionMessage {
                id: row.get(0)?,
                guid: row.get(1)?,
                body: row.get(2)?,
                date_unix: row.get(3)?,
                sender_name: row.get(4)?,
                has_attachments: row.get(5)?,
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
        "SELECT id, message_id, filename, mime_type, transfer_name, total_bytes, is_sticker
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
                    transfer_name: row.get(4)?,
                    total_bytes: row.get(5)?,
                    is_sticker: row.get::<_, bool>(6).unwrap_or(false),
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
        match self.mime_type.as_deref() {
            Some(m) if m.starts_with("image/") => "image",
            Some(m) if m.starts_with("video/") => "video",
            Some(m) if m.starts_with("audio/") => "audio",
            _ => "other",
        }
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
            format!("{} AND a.mime_type LIKE ?1", base_where),
            vec![Box::new("image/%".to_string())],
        ),
        Some("video") => (
            format!("{} AND a.mime_type LIKE ?1", base_where),
            vec![Box::new("video/%".to_string())],
        ),
        Some("audio") => (
            format!("{} AND a.mime_type LIKE ?1", base_where),
            vec![Box::new("audio/%".to_string())],
        ),
        Some("other") => (
            format!("{} AND a.mime_type NOT LIKE 'image/%' AND a.mime_type NOT LIKE 'video/%' AND a.mime_type NOT LIKE 'audio/%'", base_where),
            vec![],
        ),
        _ => (base_where.to_string(), vec![]),
    };

    let sql = format!(
        "SELECT a.id, a.filename, a.mime_type, a.total_bytes, a.resolved_path,
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
         ORDER BY m.date_unix ASC
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
                total_bytes: row.get(3)?,
                resolved_path: row.get(4)?,
                file_exists: row.get(5)?,
                transfer_name: row.get(6)?,
                conversation_name: row.get(7)?,
                message_date: row.get(8)?,
                conversation_id: row.get(9)?,
                ck_sync_state: row.get(10)?,
                backup_source_path: row.get(11)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

pub fn get_attachment(conn: &Connection, id: i64) -> anyhow::Result<Option<AttachmentRow>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.filename, a.mime_type, a.total_bytes, a.resolved_path,
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
                total_bytes: row.get(3)?,
                resolved_path: row.get(4)?,
                file_exists: row.get(5)?,
                transfer_name: row.get(6)?,
                conversation_name: row.get(7)?,
                message_date: row.get(8)?,
                conversation_id: row.get(9)?,
                ck_sync_state: row.get(10)?,
                backup_source_path: row.get(11)?,
            })
        })
        .optional()?;

    Ok(row)
}

pub fn count_attachments(conn: &Connection, mime_filter: Option<&str>) -> anyhow::Result<i64> {
    let base_where = "(filename IS NULL OR filename NOT LIKE '%.pluginPayloadAttachment') AND (transfer_name IS NULL OR transfer_name NOT LIKE '%.pluginPayloadAttachment')";
    let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match mime_filter {
        Some("image") => (
            format!("SELECT COUNT(*) FROM attachments WHERE {} AND mime_type LIKE ?1", base_where),
            vec![Box::new("image/%".to_string())],
        ),
        Some("video") => (
            format!("SELECT COUNT(*) FROM attachments WHERE {} AND mime_type LIKE ?1", base_where),
            vec![Box::new("video/%".to_string())],
        ),
        Some("audio") => (
            format!("SELECT COUNT(*) FROM attachments WHERE {} AND mime_type LIKE ?1", base_where),
            vec![Box::new("audio/%".to_string())],
        ),
        Some("other") => (
            format!("SELECT COUNT(*) FROM attachments WHERE {} AND mime_type NOT LIKE 'image/%' AND mime_type NOT LIKE 'video/%' AND mime_type NOT LIKE 'audio/%'", base_where),
            vec![],
        ),
        _ => (format!("SELECT COUNT(*) FROM attachments WHERE {}", base_where), vec![]),
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
        "SELECT a.id, a.filename, a.mime_type, a.total_bytes, a.resolved_path,
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
         ORDER BY m.date_unix ASC
         LIMIT {limit} OFFSET {offset}"
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([conversation_id], |row| {
            Ok(AttachmentRow {
                id: row.get(0)?,
                filename: row.get(1)?,
                mime_type: row.get(2)?,
                total_bytes: row.get(3)?,
                resolved_path: row.get(4)?,
                file_exists: row.get(5)?,
                transfer_name: row.get(6)?,
                conversation_name: row.get(7)?,
                message_date: row.get(8)?,
                conversation_id: row.get(9)?,
                ck_sync_state: row.get(10)?,
                backup_source_path: row.get(11)?,
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
        "SELECT a.id, a.filename, a.mime_type, a.total_bytes, a.resolved_path,
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
                total_bytes: row.get(3)?,
                resolved_path: row.get(4)?,
                file_exists: row.get(5)?,
                transfer_name: row.get(6)?,
                conversation_name: row.get(7)?,
                message_date: row.get(8)?,
                conversation_id: row.get(9)?,
                ck_sync_state: row.get(10)?,
                backup_source_path: row.get(11)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
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
