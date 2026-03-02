use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Contact {
    pub id: i64,
    pub handle: String,
    pub display_name: Option<String>,
    pub service: Option<String>,
    pub person_centric_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Conversation {
    pub id: i64,
    pub apple_chat_id: Option<i64>,
    pub guid: Option<String>,
    pub display_name: Option<String>,
    pub is_group: bool,
    pub service: Option<String>,
    pub last_message_date: Option<i64>,
    pub message_count: i64,
    pub participant_count: i64,
}

#[derive(Debug, Serialize)]
pub struct ConversationParticipant {
    pub conversation_id: i64,
    pub contact_id: i64,
}

#[derive(Debug, Serialize)]
pub struct Message {
    pub id: i64,
    pub apple_message_id: Option<i64>,
    pub guid: Option<String>,
    pub conversation_id: i64,
    pub sender_id: Option<i64>,
    pub is_from_me: bool,
    pub body: Option<String>,
    pub date_unix: i64,
    pub service: Option<String>,
    pub is_reaction: bool,
    pub reaction_type: Option<i64>,
    pub thread_originator_guid: Option<String>,
    pub is_edited: bool,
    pub has_attachments: bool,
    pub balloon_bundle_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Attachment {
    pub id: i64,
    pub message_id: i64,
    pub apple_attachment_id: Option<i64>,
    pub guid: Option<String>,
    pub filename: Option<String>,
    pub resolved_path: Option<String>,
    pub mime_type: Option<String>,
    pub uti: Option<String>,
    pub transfer_name: Option<String>,
    pub total_bytes: Option<i64>,
    pub file_exists: bool,
}
