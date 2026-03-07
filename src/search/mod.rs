use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub id: i64,
    pub body: Option<String>,
    pub highlighted_body: Option<String>,
    pub date_unix: i64,
    pub is_from_me: bool,
    pub conversation_id: i64,
    pub conversation_name: Option<String>,
    pub sender_name: Option<String>,
    pub sender_handle: Option<String>,
}

const FTS_QUERY: &str = "
SELECT m.id, m.body, m.date_unix, m.is_from_me, m.conversation_id,
       c.display_name AS conversation_name,
       ct.display_name AS sender_name, ct.handle AS sender_handle,
       highlight(messages_fts, 0, '<mark>', '</mark>') AS highlighted
FROM messages_fts
JOIN messages m ON m.id = messages_fts.rowid
JOIN conversations c ON c.id = m.conversation_id
LEFT JOIN contacts ct ON ct.id = m.sender_id
WHERE messages_fts MATCH ?1
  AND m.is_reaction = FALSE
ORDER BY m.date_unix DESC
LIMIT ?2 OFFSET ?3
";

const LIKE_QUERY: &str = "
SELECT m.id, m.body, m.date_unix, m.is_from_me, m.conversation_id,
       c.display_name AS conversation_name,
       ct.display_name AS sender_name, ct.handle AS sender_handle
FROM messages m
JOIN conversations c ON c.id = m.conversation_id
LEFT JOIN contacts ct ON ct.id = m.sender_id
WHERE m.body LIKE '%' || ?1 || '%'
  AND m.is_reaction = FALSE
ORDER BY m.date_unix DESC
LIMIT ?2 OFFSET ?3
";

const FTS_COUNT_QUERY: &str = "
SELECT COUNT(*)
FROM messages_fts
JOIN messages m ON m.id = messages_fts.rowid
WHERE messages_fts MATCH ?1
  AND m.is_reaction = FALSE
";

const LIKE_COUNT_QUERY: &str = "
SELECT COUNT(*)
FROM messages m
WHERE m.body LIKE '%' || ?1 || '%'
  AND m.is_reaction = FALSE
";

pub fn search(
    conn: &Connection,
    query: &str,
    limit: usize,
    offset: usize,
) -> anyhow::Result<Vec<SearchResult>> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    if trimmed.len() >= 3 {
        search_fts(conn, trimmed, limit, offset)
    } else {
        search_like(conn, trimmed, limit, offset)
    }
}

fn search_fts(
    conn: &Connection,
    query: &str,
    limit: usize,
    offset: usize,
) -> anyhow::Result<Vec<SearchResult>> {
    let escaped = escape_fts_query(query);
    let mut stmt = conn.prepare(FTS_QUERY)?;
    let rows = stmt.query_map(
        rusqlite::params![escaped, limit as i64, offset as i64],
        |row| {
            Ok(SearchResult {
                id: row.get(0)?,
                body: row.get(1)?,
                date_unix: row.get(2)?,
                is_from_me: row.get(3)?,
                conversation_id: row.get(4)?,
                conversation_name: row.get(5)?,
                sender_name: row.get(6)?,
                sender_handle: row.get(7)?,
                highlighted_body: row.get(8)?,
            })
        },
    )?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

fn search_like(
    conn: &Connection,
    query: &str,
    limit: usize,
    offset: usize,
) -> anyhow::Result<Vec<SearchResult>> {
    let mut stmt = conn.prepare(LIKE_QUERY)?;
    let rows = stmt.query_map(
        rusqlite::params![query, limit as i64, offset as i64],
        |row| {
            let body: Option<String> = row.get(1)?;
            Ok(SearchResult {
                id: row.get(0)?,
                body: body.clone(),
                date_unix: row.get(2)?,
                is_from_me: row.get(3)?,
                conversation_id: row.get(4)?,
                conversation_name: row.get(5)?,
                sender_name: row.get(6)?,
                sender_handle: row.get(7)?,
                highlighted_body: body,
            })
        },
    )?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

pub fn search_count(conn: &Connection, query: &str) -> anyhow::Result<usize> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(0);
    }

    if trimmed.len() >= 3 {
        let escaped = escape_fts_query(trimmed);
        let count: i64 = conn.query_row(FTS_COUNT_QUERY, [&escaped], |row| row.get(0))?;
        Ok(count as usize)
    } else {
        let count: i64 = conn.query_row(LIKE_COUNT_QUERY, [trimmed], |row| row.get(0))?;
        Ok(count as usize)
    }
}

fn escape_fts_query(query: &str) -> String {
    format!("\"{}\"", query.replace('"', "\"\""))
}
