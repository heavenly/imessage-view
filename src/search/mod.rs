use chrono::{Duration, Local, NaiveDate, TimeZone};
use rusqlite::{params_from_iter, types::ToSql, Connection, Row};
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

#[derive(Debug, Default, PartialEq, Eq)]
struct ParsedSearchQuery {
    body_query: Option<String>,
    from_filter: Option<String>,
    in_filter: Option<String>,
    after_unix: Option<i64>,
    before_unix: Option<i64>,
}

impl ParsedSearchQuery {
    fn has_filters(&self) -> bool {
        self.from_filter.is_some()
            || self.in_filter.is_some()
            || self.after_unix.is_some()
            || self.before_unix.is_some()
    }

    fn is_empty(&self) -> bool {
        self.body_query.is_none() && !self.has_filters()
    }
}

pub fn search(
    conn: &Connection,
    query: &str,
    limit: usize,
    offset: usize,
) -> anyhow::Result<Vec<SearchResult>> {
    let parsed = parse_search_query(query);
    if parsed.is_empty() {
        return Ok(Vec::new());
    }

    if let Some(body_query) = parsed.body_query.as_deref() {
        if body_query.len() >= 3 {
            search_fts(conn, &parsed, limit, offset)
        } else {
            search_like(conn, &parsed, limit, offset)
        }
    } else {
        search_filtered(conn, &parsed, limit, offset)
    }
}

pub fn search_count(conn: &Connection, query: &str) -> anyhow::Result<usize> {
    let parsed = parse_search_query(query);
    if parsed.is_empty() {
        return Ok(0);
    }

    let (sql, params) = if let Some(body_query) = parsed.body_query.as_deref() {
        if body_query.len() >= 3 {
            build_fts_count_query(&parsed)
        } else {
            build_non_fts_count_query(&parsed, true)
        }
    } else {
        build_non_fts_count_query(&parsed, false)
    };

    let param_refs: Vec<&dyn ToSql> = params.iter().map(|value| value.as_ref()).collect();
    let count: i64 = conn.query_row(&sql, params_from_iter(param_refs), |row| row.get(0))?;
    Ok(count as usize)
}

fn search_fts(
    conn: &Connection,
    parsed: &ParsedSearchQuery,
    limit: usize,
    offset: usize,
) -> anyhow::Result<Vec<SearchResult>> {
    let (sql, mut params) = build_fts_search_query(parsed);
    params.push(Box::new(limit as i64));
    params.push(Box::new(offset as i64));
    query_search_results(conn, &sql, params, true)
}

fn search_like(
    conn: &Connection,
    parsed: &ParsedSearchQuery,
    limit: usize,
    offset: usize,
) -> anyhow::Result<Vec<SearchResult>> {
    let (sql, mut params) = build_non_fts_search_query(parsed, true);
    params.push(Box::new(limit as i64));
    params.push(Box::new(offset as i64));
    query_search_results(conn, &sql, params, false)
}

fn search_filtered(
    conn: &Connection,
    parsed: &ParsedSearchQuery,
    limit: usize,
    offset: usize,
) -> anyhow::Result<Vec<SearchResult>> {
    let (sql, mut params) = build_non_fts_search_query(parsed, false);
    params.push(Box::new(limit as i64));
    params.push(Box::new(offset as i64));
    query_search_results(conn, &sql, params, false)
}

fn query_search_results(
    conn: &Connection,
    sql: &str,
    params: Vec<Box<dyn ToSql>>,
    has_highlight: bool,
) -> anyhow::Result<Vec<SearchResult>> {
    let mut stmt = conn.prepare(sql)?;
    let param_refs: Vec<&dyn ToSql> = params.iter().map(|value| value.as_ref()).collect();
    let rows = stmt.query_map(params_from_iter(param_refs), |row| {
        map_search_result_row(row, has_highlight)
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

fn map_search_result_row(row: &Row, has_highlight: bool) -> rusqlite::Result<SearchResult> {
    Ok(SearchResult {
        id: row.get(0)?,
        body: row.get(1)?,
        date_unix: row.get(2)?,
        is_from_me: row.get(3)?,
        conversation_id: row.get(4)?,
        conversation_name: row.get(5)?,
        sender_name: row.get(6)?,
        sender_handle: row.get(7)?,
        highlighted_body: if has_highlight {
            row.get(8)?
        } else {
            row.get(1)?
        },
    })
}

fn build_fts_search_query(parsed: &ParsedSearchQuery) -> (String, Vec<Box<dyn ToSql>>) {
    let mut sql = String::from(
        "SELECT m.id, m.body, m.date_unix, m.is_from_me, m.conversation_id,
                c.display_name AS conversation_name,
                ct.display_name AS sender_name, ct.handle AS sender_handle,
                highlight(messages_fts, 0, '<mark>', '</mark>') AS highlighted
         FROM messages_fts
         JOIN messages m ON m.id = messages_fts.rowid
         JOIN conversations c ON c.id = m.conversation_id
         LEFT JOIN contacts ct ON ct.id = m.sender_id
         WHERE messages_fts MATCH ",
    );
    let mut params: Vec<Box<dyn ToSql>> = Vec::new();
    let body_query = escape_fts_query(parsed.body_query.as_deref().unwrap_or_default());
    let body_placeholder = push_param(&mut params, body_query);
    sql.push_str(&body_placeholder);
    append_common_filters(&mut sql, &mut params, parsed, false);
    sql.push_str(" ORDER BY m.date_unix DESC, m.id DESC");
    let limit_placeholder = next_placeholder(params.len() + 1);
    let offset_placeholder = next_placeholder(params.len() + 2);
    sql.push_str(&format!(
        " LIMIT {limit_placeholder} OFFSET {offset_placeholder}"
    ));
    (sql, params)
}

fn build_non_fts_search_query(
    parsed: &ParsedSearchQuery,
    include_body_filter: bool,
) -> (String, Vec<Box<dyn ToSql>>) {
    let mut sql = String::from(
        "SELECT m.id, m.body, m.date_unix, m.is_from_me, m.conversation_id,
                c.display_name AS conversation_name,
                ct.display_name AS sender_name, ct.handle AS sender_handle
         FROM messages m
         JOIN conversations c ON c.id = m.conversation_id
         LEFT JOIN contacts ct ON ct.id = m.sender_id
         WHERE 1 = 1",
    );
    let mut params: Vec<Box<dyn ToSql>> = Vec::new();
    if include_body_filter {
        if let Some(body_query) = parsed.body_query.as_deref() {
            let placeholder = push_param(&mut params, body_query.to_string());
            sql.push_str(&format!(" AND m.body LIKE '%' || {placeholder} || '%'"));
        }
    }
    append_common_filters(&mut sql, &mut params, parsed, include_body_filter);
    sql.push_str(" ORDER BY m.date_unix DESC, m.id DESC");
    let limit_placeholder = next_placeholder(params.len() + 1);
    let offset_placeholder = next_placeholder(params.len() + 2);
    sql.push_str(&format!(
        " LIMIT {limit_placeholder} OFFSET {offset_placeholder}"
    ));
    (sql, params)
}

fn build_fts_count_query(parsed: &ParsedSearchQuery) -> (String, Vec<Box<dyn ToSql>>) {
    let mut sql = String::from(
        "SELECT COUNT(*)
         FROM messages_fts
         JOIN messages m ON m.id = messages_fts.rowid
         JOIN conversations c ON c.id = m.conversation_id
         LEFT JOIN contacts ct ON ct.id = m.sender_id
         WHERE messages_fts MATCH ",
    );
    let mut params: Vec<Box<dyn ToSql>> = Vec::new();
    let body_query = escape_fts_query(parsed.body_query.as_deref().unwrap_or_default());
    let body_placeholder = push_param(&mut params, body_query);
    sql.push_str(&body_placeholder);
    append_common_filters(&mut sql, &mut params, parsed, false);
    (sql, params)
}

fn build_non_fts_count_query(
    parsed: &ParsedSearchQuery,
    include_body_filter: bool,
) -> (String, Vec<Box<dyn ToSql>>) {
    let mut sql = String::from(
        "SELECT COUNT(*)
         FROM messages m
         JOIN conversations c ON c.id = m.conversation_id
         LEFT JOIN contacts ct ON ct.id = m.sender_id
         WHERE 1 = 1",
    );
    let mut params: Vec<Box<dyn ToSql>> = Vec::new();
    if include_body_filter {
        if let Some(body_query) = parsed.body_query.as_deref() {
            let placeholder = push_param(&mut params, body_query.to_string());
            sql.push_str(&format!(" AND m.body LIKE '%' || {placeholder} || '%'"));
        }
    }
    append_common_filters(&mut sql, &mut params, parsed, include_body_filter);
    (sql, params)
}

fn append_common_filters(
    sql: &mut String,
    params: &mut Vec<Box<dyn ToSql>>,
    parsed: &ParsedSearchQuery,
    has_body_like_filter: bool,
) {
    if !has_body_like_filter {
        sql.push_str(" AND m.is_reaction = FALSE");
    } else {
        sql.push_str(" AND m.is_reaction = FALSE");
    }

    if let Some(from_filter) = parsed.from_filter.as_deref() {
        let from_placeholder = push_param(params, like_pattern(from_filter));
        let mut clauses = vec![
            format!("LOWER(COALESCE(ct.display_name, '')) LIKE LOWER({from_placeholder})"),
            format!("LOWER(COALESCE(ct.handle, '')) LIKE LOWER({from_placeholder})"),
        ];

        let normalized_phone = normalize_phone(from_filter);
        if !normalized_phone.is_empty() {
            let phone_placeholder = push_param(params, normalized_phone);
            clauses.push(format!(
                "{} LIKE '%' || {} || '%'",
                normalized_handle_sql("COALESCE(ct.handle, '')"),
                phone_placeholder
            ));
        }

        if from_filter.trim().eq_ignore_ascii_case("me") {
            clauses.push("m.is_from_me = TRUE".to_string());
        }

        sql.push_str(" AND (");
        sql.push_str(&clauses.join(" OR "));
        sql.push(')');
    }

    if let Some(in_filter) = parsed.in_filter.as_deref() {
        let in_placeholder = push_param(params, like_pattern(in_filter));
        sql.push_str(&format!(
            " AND LOWER(COALESCE(c.display_name, '')) LIKE LOWER({in_placeholder})"
        ));
    }

    if let Some(after_unix) = parsed.after_unix {
        let after_placeholder = push_param(params, after_unix);
        sql.push_str(&format!(" AND m.date_unix >= {after_placeholder}"));
    }

    if let Some(before_unix) = parsed.before_unix {
        let before_placeholder = push_param(params, before_unix);
        sql.push_str(&format!(" AND m.date_unix < {before_placeholder}"));
    }
}

fn push_param<T>(params: &mut Vec<Box<dyn ToSql>>, value: T) -> String
where
    T: ToSql + 'static,
{
    params.push(Box::new(value));
    next_placeholder(params.len())
}

fn next_placeholder(index: usize) -> String {
    format!("?{index}")
}

fn like_pattern(value: &str) -> String {
    format!("%{}%", value.trim())
}

fn normalized_handle_sql(expr: &str) -> String {
    let digits = format!(
        "REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE({expr}, ' ', ''), '-', ''), '(', ''), ')', ''), '+', ''), '.', '')"
    );
    format!(
        "CASE WHEN LENGTH({digits}) = 11 AND SUBSTR({digits}, 1, 1) = '1' THEN SUBSTR({digits}, 2) ELSE {digits} END"
    )
}

fn parse_search_query(query: &str) -> ParsedSearchQuery {
    let mut parsed = ParsedSearchQuery::default();
    let mut body_tokens = Vec::new();

    for token in tokenize_query(query) {
        let Some((operator, raw_value)) = token.split_once(':') else {
            body_tokens.push(token);
            continue;
        };

        let value = parse_operator_value(raw_value);
        if value.is_empty() {
            body_tokens.push(token);
            continue;
        }

        if operator.eq_ignore_ascii_case("from") {
            parsed.from_filter = Some(value);
            continue;
        }

        if operator.eq_ignore_ascii_case("in") {
            parsed.in_filter = Some(value);
            continue;
        }

        if operator.eq_ignore_ascii_case("after") {
            if let Some(after_unix) = parse_after_date(&value) {
                parsed.after_unix = Some(after_unix);
            } else {
                body_tokens.push(token);
            }
            continue;
        }

        if operator.eq_ignore_ascii_case("before") {
            if let Some(before_unix) = parse_before_date(&value) {
                parsed.before_unix = Some(before_unix);
            } else {
                body_tokens.push(token);
            }
            continue;
        }

        body_tokens.push(token);
    }

    let body_query = body_tokens.join(" ").trim().to_string();
    if !body_query.is_empty() {
        parsed.body_query = Some(body_query);
    }

    parsed
}

fn tokenize_query(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in query.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn parse_operator_value(raw_value: &str) -> String {
    let trimmed = raw_value.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        trimmed.to_string()
    }
}

fn parse_after_date(value: &str) -> Option<i64> {
    parse_supported_date(value).and_then(local_start_of_day_unix)
}

fn parse_before_date(value: &str) -> Option<i64> {
    let next_day = parse_supported_date(value)?.checked_add_signed(Duration::days(1))?;
    local_start_of_day_unix(next_day)
}

fn parse_supported_date(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value, "%m-%d-%Y")
        .ok()
        .or_else(|| NaiveDate::parse_from_str(value, "%-m-%-d-%Y").ok())
        .or_else(|| NaiveDate::parse_from_str(value, "%m/%d/%Y").ok())
        .or_else(|| NaiveDate::parse_from_str(value, "%-m/%-d/%Y").ok())
}

fn local_start_of_day_unix(date: NaiveDate) -> Option<i64> {
    let naive = date.and_hms_opt(0, 0, 0)?;
    Local
        .from_local_datetime(&naive)
        .earliest()
        .or_else(|| Local.from_local_datetime(&naive).latest())
        .map(|dt| dt.timestamp())
}

fn normalize_phone(phone: &str) -> String {
    let digits: String = phone.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() == 11 && digits.starts_with('1') {
        digits[1..].to_string()
    } else {
        digits
    }
}

fn escape_fts_query(query: &str) -> String {
    format!("\"{}\"", query.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::db::schema;

    fn setup_search_db() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        schema::create_all_tables(&conn).expect("create schema");

        conn.execute(
            "INSERT INTO contacts (id, handle, display_name) VALUES (?1, ?2, ?3)",
            rusqlite::params![1i64, "+1 (555) 123-4567", "Jacob"],
        )
        .expect("insert Jacob");
        conn.execute(
            "INSERT INTO contacts (id, handle, display_name) VALUES (?1, ?2, ?3)",
            rusqlite::params![2i64, "sarah@example.com", "Sarah"],
        )
        .expect("insert Sarah");

        conn.execute(
            "INSERT INTO conversations (id, guid, display_name, is_group) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![1i64, "chat-1", "IDF Goy Extermination Unit", true],
        )
        .expect("insert group conversation");
        conn.execute(
            "INSERT INTO conversations (id, guid, display_name, is_group) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![2i64, "chat-2", "Other Group", true],
        )
        .expect("insert second conversation");

        conn.execute(
            "INSERT INTO messages (id, guid, conversation_id, sender_id, is_from_me, body, date_unix, is_reaction)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![1i64, "msg-1", 1i64, 1i64, false, "hello there", 1_704_153_600i64, false],
        )
        .expect("insert first message");
        conn.execute(
            "INSERT INTO messages (id, guid, conversation_id, sender_id, is_from_me, body, date_unix, is_reaction)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![2i64, "msg-2", 1i64, 2i64, false, "goodbye", 1_704_412_800i64, false],
        )
        .expect("insert second message");
        conn.execute(
            "INSERT INTO messages (id, guid, conversation_id, sender_id, is_from_me, body, date_unix, is_reaction)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![3i64, "msg-3", 2i64, 1i64, false, "hello again", 1_706_918_400i64, false],
        )
        .expect("insert third message");
        conn.execute(
            "INSERT INTO messages (id, guid, conversation_id, sender_id, is_from_me, body, date_unix, is_reaction)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![4i64, "msg-4", 1i64, Option::<i64>::None, true, "hello from me", 1_704_844_800i64, false],
        )
        .expect("insert my message");

        conn.execute(
            "INSERT INTO messages_fts(messages_fts) VALUES('rebuild')",
            [],
        )
        .expect("rebuild fts");

        conn
    }

    #[test]
    fn test_parse_search_query_extracts_structured_filters() {
        let parsed = parse_search_query(
            "from:\"Jacob\" in:\"IDF Goy Extermination Unit\" after:\"01-01-2024\" before:\"1/31/2024\" hello",
        );

        assert_eq!(parsed.body_query.as_deref(), Some("hello"));
        assert_eq!(parsed.from_filter.as_deref(), Some("Jacob"));
        assert_eq!(
            parsed.in_filter.as_deref(),
            Some("IDF Goy Extermination Unit")
        );
        assert!(parsed.after_unix.is_some(), "expected parsed after date");
        assert!(parsed.before_unix.is_some(), "expected parsed before date");
    }

    #[test]
    fn test_parse_search_query_invalid_date_falls_back_to_body_text() {
        let parsed = parse_search_query("before:\"not-a-date\" hello");

        assert_eq!(parsed.before_unix, None);
        assert_eq!(
            parsed.body_query.as_deref(),
            Some("before:\"not-a-date\" hello")
        );
    }

    #[test]
    fn test_search_filters_by_sender_conversation_and_text() {
        let conn = setup_search_db();
        let results = search(
            &conn,
            "from:\"Jacob\" in:\"IDF Goy Extermination Unit\" hello",
            20,
            0,
        )
        .expect("run structured search");

        assert_eq!(results.len(), 1, "expected one matching result");
        assert_eq!(results[0].id, 1, "expected Jacob hello in target chat");
    }

    #[test]
    fn test_search_filters_by_phone_and_date_bounds() {
        let conn = setup_search_db();
        let results = search(
            &conn,
            "from:\"(555) 123-4567\" after:\"02-01-2024\" hello",
            20,
            0,
        )
        .expect("run phone/date search");

        assert_eq!(results.len(), 1, "expected one post-February Jacob message");
        assert_eq!(results[0].id, 3, "expected the February Jacob message");
    }

    #[test]
    fn test_search_supports_filter_only_queries_and_count_parity() {
        let conn = setup_search_db();
        let results = search(&conn, "from:\"Jacob\" before:\"02-01-2024\"", 20, 0)
            .expect("run filter-only search");
        let count = search_count(&conn, "from:\"Jacob\" before:\"02-01-2024\"")
            .expect("count filter-only search");

        assert_eq!(count, results.len(), "count should match fetched rows");
        assert_eq!(
            results.len(),
            1,
            "expected one Jacob message before February"
        );
        assert_eq!(results[0].id, 1, "expected only the January Jacob message");
    }
}
