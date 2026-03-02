pub mod queries;
pub mod schema;

use std::path::Path;
use rusqlite::Connection;

fn set_pragmas(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA cache_size = -64000;",
    )?;
    Ok(())
}

pub fn create_db(path: &Path) -> anyhow::Result<Connection> {
    let conn = Connection::open(path)?;
    set_pragmas(&conn)?;
    schema::create_all_tables(&conn)?;
    Ok(conn)
}

pub fn drop_and_recreate(path: &Path) -> anyhow::Result<Connection> {
    let conn = Connection::open(path)?;
    set_pragmas(&conn)?;
    schema::drop_all_tables(&conn)?;
    schema::create_all_tables(&conn)?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_db_all_tables() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let conn = create_db(&db_path).unwrap();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type IN ('table', 'view') ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"contacts".to_string()));
        assert!(tables.contains(&"conversations".to_string()));
        assert!(tables.contains(&"conversation_participants".to_string()));
        assert!(tables.contains(&"messages".to_string()));
        assert!(tables.contains(&"attachments".to_string()));
        assert!(tables.contains(&"messages_fts".to_string()));
    }

    #[test]
    fn test_fts5_trigram_tokenizer() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let conn = create_db(&db_path).unwrap();

        let sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'messages_fts'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert!(sql.contains("trigram"), "FTS5 should use trigram tokenizer, got: {sql}");
    }

    #[test]
    fn test_indexes_created() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let conn = create_db(&db_path).unwrap();

        let indexes: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'index' AND name LIKE 'idx_%'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(indexes.contains(&"idx_messages_conversation_date".to_string()));
        assert!(indexes.contains(&"idx_messages_sender".to_string()));
        assert!(indexes.contains(&"idx_messages_date".to_string()));
        assert!(indexes.contains(&"idx_attachments_message".to_string()));
        assert!(indexes.contains(&"idx_attachments_mime".to_string()));
        assert!(indexes.contains(&"idx_contacts_handle".to_string()));
    }

    #[test]
    fn test_drop_and_recreate() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let _conn = create_db(&db_path).unwrap();
        let conn = drop_and_recreate(&db_path).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type IN ('table', 'view')",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert!(count >= 6, "Expected at least 6 tables/views, got {count}");
    }

    #[test]
    fn test_pragmas_set() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let conn = create_db(&db_path).unwrap();

        let journal: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal, "wal");

        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }
}
