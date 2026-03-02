use rusqlite::Connection;

pub const CREATE_CONTACTS: &str = "
CREATE TABLE IF NOT EXISTS contacts (
    id INTEGER PRIMARY KEY,
    handle TEXT NOT NULL UNIQUE,
    display_name TEXT,
    service TEXT,
    person_centric_id TEXT
);";

pub const CREATE_CONVERSATIONS: &str = "
CREATE TABLE IF NOT EXISTS conversations (
    id INTEGER PRIMARY KEY,
    apple_chat_id INTEGER,
    guid TEXT UNIQUE,
    display_name TEXT,
    is_group BOOLEAN NOT NULL,
    service TEXT,
    last_message_date INTEGER,
    message_count INTEGER DEFAULT 0,
    participant_count INTEGER DEFAULT 0
);";

pub const CREATE_CONVERSATION_PARTICIPANTS: &str = "
CREATE TABLE IF NOT EXISTS conversation_participants (
    conversation_id INTEGER REFERENCES conversations(id),
    contact_id INTEGER REFERENCES contacts(id),
    PRIMARY KEY (conversation_id, contact_id)
);";

pub const CREATE_MESSAGES: &str = "
CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY,
    apple_message_id INTEGER UNIQUE,
    guid TEXT UNIQUE,
    conversation_id INTEGER NOT NULL REFERENCES conversations(id),
    sender_id INTEGER REFERENCES contacts(id),
    is_from_me BOOLEAN NOT NULL,
    body TEXT,
    date_unix INTEGER NOT NULL,
    service TEXT,
    is_reaction BOOLEAN DEFAULT FALSE,
    reaction_type INTEGER,
    thread_originator_guid TEXT,
    is_edited BOOLEAN DEFAULT FALSE,
    has_attachments BOOLEAN DEFAULT FALSE,
    balloon_bundle_id TEXT
);";

pub const CREATE_ATTACHMENTS: &str = "
CREATE TABLE IF NOT EXISTS attachments (
    id INTEGER PRIMARY KEY,
    message_id INTEGER NOT NULL REFERENCES messages(id),
    apple_attachment_id INTEGER,
    guid TEXT,
    filename TEXT,
    resolved_path TEXT,
    mime_type TEXT,
    uti TEXT,
    transfer_name TEXT,
    total_bytes INTEGER,
    file_exists BOOLEAN DEFAULT FALSE
);";

pub const CREATE_MESSAGES_FTS: &str = "
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    body, content='messages', content_rowid='id', tokenize='trigram'
);";

pub const CREATE_INDEXES: &[&str] = &[
    "CREATE INDEX IF NOT EXISTS idx_messages_conversation_date ON messages(conversation_id, date_unix DESC)",
    "CREATE INDEX IF NOT EXISTS idx_messages_sender ON messages(sender_id)",
    "CREATE INDEX IF NOT EXISTS idx_messages_date ON messages(date_unix DESC)",
    "CREATE INDEX IF NOT EXISTS idx_attachments_message ON attachments(message_id)",
    "CREATE INDEX IF NOT EXISTS idx_attachments_mime ON attachments(mime_type)",
    "CREATE INDEX IF NOT EXISTS idx_contacts_handle ON contacts(handle)",
];

pub fn create_all_tables(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(CREATE_CONTACTS)?;
    conn.execute_batch(CREATE_CONVERSATIONS)?;
    conn.execute_batch(CREATE_CONVERSATION_PARTICIPANTS)?;
    conn.execute_batch(CREATE_MESSAGES)?;
    conn.execute_batch(CREATE_ATTACHMENTS)?;
    conn.execute_batch(CREATE_MESSAGES_FTS)?;
    for idx in CREATE_INDEXES {
        conn.execute_batch(idx)?;
    }
    Ok(())
}

pub fn drop_all_tables(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "DROP TABLE IF EXISTS messages_fts;
         DROP TABLE IF EXISTS attachments;
         DROP TABLE IF EXISTS messages;
         DROP TABLE IF EXISTS conversation_participants;
         DROP TABLE IF EXISTS conversations;
         DROP TABLE IF EXISTS contacts;",
    )?;
    Ok(())
}
