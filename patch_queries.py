import sys

with open("src/db/queries.rs", "r") as f:
    content = f.read()

new_queries = """
pub fn count_missing_attachments(conn: &Connection) -> anyhow::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM attachments WHERE file_exists = 0",
        [],
        |r| r.get(0),
    )
    .map_err(Into::into)
}

pub fn count_missing_icloud_attachments(conn: &Connection) -> anyhow::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM attachments WHERE file_exists = 0 AND ck_sync_state = 1",
        [],
        |r| r.get(0),
    )
    .map_err(Into::into)
}

pub fn count_missing_with_backup(conn: &Connection) -> anyhow::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM attachments WHERE file_exists = 0 AND backup_source_path IS NOT NULL",
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
         ORDER BY m.date_unix DESC
         LIMIT {limit} OFFSET {offset}"
    );

    let mut stmt = conn.prepare(&sql)?;

    let rows = stmt
        .query_map([], |row| {"""

old_func_start = """pub fn get_missing_attachments(conn: &Connection) -> anyhow::Result<Vec<AttachmentRow>> {
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
         WHERE a.file_exists = 0",
    )?;

    let rows = stmt
        .query_map([], |row| {"""

if old_func_start in content:
    content = content.replace(old_func_start, new_queries)
    with open("src/db/queries.rs", "w") as f:
        f.write(content)
    print("Replaced successfully")
else:
    print("Pattern not found")
