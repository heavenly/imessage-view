use askama::Template;
use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse};
use chrono::DateTime;
use serde::Deserialize;

use crate::db::queries;
use crate::search;
use crate::state::AppState;

use super::pages::ConversationRow;

#[derive(Deserialize)]
pub struct ConversationPanelQuery {
    pub id: i64,
}

#[derive(Debug, Clone)]
pub struct ContributionDay {
    pub date: String,
    pub level: u8,
}

pub fn build_contribution_graph(
    conn: &rusqlite::Connection,
    conversation_id: i64,
) -> Vec<ContributionDay> {
    use std::collections::HashMap;
    let rows = queries::get_mutual_interaction_days(conn, conversation_id, 90).unwrap_or_default();
    let day_map: HashMap<String, &queries::MutualInteractionDay> =
        rows.iter().map(|d| (d.date.clone(), d)).collect();

    let max_total = day_map
        .values()
        .filter(|d| d.sent > 0 && d.received > 0)
        .map(|d| d.sent + d.received)
        .max()
        .unwrap_or(1) as f64;

    let today = chrono::Utc::now().date_naive();
    let mut result = Vec::with_capacity(90);
    for i in (0..90).rev() {
        let date = today - chrono::Duration::days(i);
        let date_str = date.format("%Y-%m-%d").to_string();
        let level = match day_map.get(&date_str) {
            Some(d) if d.sent > 0 && d.received > 0 => {
                let ratio = (d.sent + d.received) as f64 / max_total;
                if ratio > 0.75 {
                    4
                } else if ratio > 0.50 {
                    3
                } else if ratio > 0.25 {
                    2
                } else {
                    1
                }
            }
            _ => 0,
        };
        result.push(ContributionDay {
            date: date_str,
            level,
        });
    }
    result
}

#[derive(Template)]
#[template(path = "partials/conversation_panel.html")]
struct ConversationPanelTemplate {
    conversation_id: i64,
    contact_name: String,
    is_group: bool,
    participants: Vec<String>,
    attachment_count: i64,
    has_photo: bool,
    contribution_days: Vec<ContributionDay>,
}

pub async fn conversation_panel_partial(
    State(state): State<AppState>,
    Query(params): Query<ConversationPanelQuery>,
) -> impl IntoResponse {
    let id = params.id;
    let conn = state.db.lock().unwrap();
    let info = queries::get_conversation_info(&conn, id);

    let (contact_name, is_group, participants, has_photo) = match info {
        Ok(info) => {
            let name = info
                .display_name
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    if info.participant_names.is_empty() {
                        "Unknown".to_string()
                    } else {
                        info.participant_names.join(", ")
                    }
                });
            (name, info.is_group, info.participant_names, info.has_photo)
        }
        Err(_) => ("Unknown".to_string(), false, vec![], false),
    };

    let attachment_count = queries::count_conversation_attachments(&conn, id).unwrap_or(0);
    let contribution_days = build_contribution_graph(&conn, id);

    let t = ConversationPanelTemplate {
        conversation_id: id,
        contact_name,
        is_group,
        participants,
        attachment_count,
        has_photo,
        contribution_days,
    };
    Html(t.render().unwrap_or_default())
}

#[derive(Deserialize)]
pub struct MessagesQuery {
    pub conversation_id: Option<i64>,
    pub page: Option<u32>,
}

struct MessageView {
    id: i64,
    body: Option<String>,
    is_from_me: bool,
    service: Option<String>,
    sender_name: Option<String>,
    has_attachments: bool,
    time_formatted: String,
    date_formatted: String,
    attachments: Vec<AttachmentView>,
}

struct MessageGroup {
    is_from_me: bool,
    date_separator: Option<String>,
    messages: Vec<MessageView>,
}

struct AttachmentView {
    id: i64,
    mime_type: Option<String>,
    transfer_name: Option<String>,
    total_bytes: Option<i64>,
}

#[derive(Template)]
#[template(path = "partials/messages.html")]
struct MessagesPartialTemplate {
    groups: Vec<MessageGroup>,
    conversation_id: i64,
    page: u32,
    has_more: bool,
    is_empty: bool,
    is_group: bool,
}

const MESSAGES_PER_PAGE: u32 = 50;

pub async fn messages_partial(
    State(state): State<AppState>,
    Query(params): Query<MessagesQuery>,
) -> impl IntoResponse {
    let conversation_id = params.conversation_id.unwrap_or(0);
    let page = params.page.unwrap_or(0);

    let conn = state.db.lock().unwrap();
    let is_group: bool = conn
        .query_row(
            "SELECT is_group FROM conversations WHERE id = ?1",
            [conversation_id],
            |r| r.get(0),
        )
        .unwrap_or(false);
    let rows = queries::get_messages(&conn, conversation_id, page, MESSAGES_PER_PAGE + 1)
        .unwrap_or_default();

    let has_more = rows.len() > MESSAGES_PER_PAGE as usize;
    let rows: Vec<_> = rows.into_iter().take(MESSAGES_PER_PAGE as usize).collect();

    let message_ids: Vec<i64> = rows.iter().filter(|m| m.has_attachments).map(|m| m.id).collect();
    let mut att_map = queries::get_message_attachments(&conn, &message_ids).unwrap_or_default();

    let mut messages: Vec<MessageView> = rows
        .into_iter()
        .map(|m| {
            let dt = DateTime::from_timestamp(m.date_unix, 0);
            let time_formatted = dt
                .map(|d| d.format("%H:%M").to_string())
                .unwrap_or_default();
            let date_formatted = dt
                .map(|d| d.format("%b %d, %Y").to_string())
                .unwrap_or_default();
            let attachments = att_map
                .remove(&m.id)
                .unwrap_or_default()
                .into_iter()
                .map(|a| AttachmentView {
                    id: a.id,
                    mime_type: a.mime_type,
                    transfer_name: a.transfer_name,
                    total_bytes: a.total_bytes,
                })
                .collect();
            MessageView {
                id: m.id,
                body: m.body,
                is_from_me: m.is_from_me,
                service: m.service,
                sender_name: m.sender_name,
                has_attachments: m.has_attachments,
                time_formatted,
                date_formatted,
                attachments,
            }
        })
        .collect();

    // Reverse: query is DESC (newest first), reverse to put oldest at top, newest at bottom
    messages.reverse();

    // Group consecutive messages by sender and date
    let is_empty = messages.is_empty();
    let mut groups: Vec<MessageGroup> = Vec::new();
    let mut last_date = String::new();
    let mut last_is_from_me: Option<bool> = None;
    let mut last_sender_name: Option<String> = None;

    for msg in messages {
        let date_changed = msg.date_formatted != last_date;
        let sender_changed = if msg.is_from_me {
            last_is_from_me != Some(true)
        } else {
            last_is_from_me != Some(false) || last_sender_name.as_ref() != msg.sender_name.as_ref()
        };

        if date_changed || sender_changed {
            let date_separator = if date_changed {
                Some(msg.date_formatted.clone())
            } else {
                None
            };
            last_date = msg.date_formatted.clone();
            last_is_from_me = Some(msg.is_from_me);
            last_sender_name = msg.sender_name.clone();
            groups.push(MessageGroup {
                is_from_me: msg.is_from_me,
                date_separator,
                messages: vec![msg],
            });
        } else {
            groups.last_mut().unwrap().messages.push(msg);
        }
    }

    let t = MessagesPartialTemplate {
        groups,
        conversation_id,
        page,
        has_more,
        is_empty,
        is_group,
    };
    Html(t.render().unwrap_or_default())
}

#[derive(Deserialize)]
pub struct ConversationsQuery {
    pub filter: Option<String>,
}

#[derive(Template)]
#[template(path = "partials/conversations.html")]
struct ConversationsPartialTemplate {
    conversations: Vec<ConversationRow>,
}

pub async fn conversations_partial(
    State(state): State<AppState>,
    Query(params): Query<ConversationsQuery>,
) -> impl IntoResponse {
    let filter_ref = params.filter.as_deref().filter(|s| !s.is_empty());
    let conversations = super::pages::build_conversation_rows(&state, filter_ref);
    let t = ConversationsPartialTemplate { conversations };
    Html(t.render().unwrap_or_default())
}

#[derive(Deserialize)]
pub struct SearchResultsQuery {
    pub q: Option<String>,
    pub page: Option<u32>,
}

const SEARCH_PAGE_SIZE: usize = 20;

struct SearchResultView {
    sender_label: String,
    conversation_id: i64,
    conversation_label: String,
    date_formatted: String,
    snippet: String,
}

#[derive(Template)]
#[template(path = "partials/search_results.html")]
struct SearchResultsPartialTemplate {
    query: String,
    results: Vec<SearchResultView>,
    total_count: usize,
    has_more: bool,
    next_page: u32,
}

pub async fn search_results_partial(
    State(state): State<AppState>,
    Query(params): Query<SearchResultsQuery>,
) -> impl IntoResponse {
    let query = params.q.unwrap_or_default();
    let page = params.page.unwrap_or(0);
    let offset = page as usize * SEARCH_PAGE_SIZE;

    if query.trim().is_empty() {
        let t = SearchResultsPartialTemplate {
            query: String::new(),
            results: Vec::new(),
            total_count: 0,
            has_more: false,
            next_page: 0,
        };
        return Html(t.render().unwrap_or_default());
    }

    let conn = state.db.lock().unwrap();
    let results = search::search(&conn, &query, SEARCH_PAGE_SIZE, offset).unwrap_or_default();
    let total_count = search::search_count(&conn, &query).unwrap_or(0);

    let views: Vec<SearchResultView> = results
        .into_iter()
        .map(|r| {
            let sender_label = if r.is_from_me {
                "Me".to_string()
            } else {
                r.sender_name
                    .or(r.sender_handle)
                    .unwrap_or_else(|| "Unknown".to_string())
            };
            let conversation_label = r
                .conversation_name
                .unwrap_or_else(|| format!("Conversation {}", r.conversation_id));
            let snippet = r.highlighted_body.or(r.body).unwrap_or_default();
            SearchResultView {
                sender_label,
                conversation_id: r.conversation_id,
                conversation_label,
                date_formatted: DateTime::from_timestamp(r.date_unix, 0)
                    .map(|dt| dt.format("%b %d, %Y %H:%M").to_string())
                    .unwrap_or_else(|| "Unknown date".to_string()),
                snippet,
            }
        })
        .collect();

    let fetched = offset + views.len();
    let has_more = fetched < total_count;

    let t = SearchResultsPartialTemplate {
        query,
        results: views,
        total_count,
        has_more,
        next_page: page + 1,
    };
    Html(t.render().unwrap_or_default())
}

#[derive(Deserialize)]
pub struct ConversationAttachmentsQuery {
    pub conversation_id: i64,
    pub page: Option<u32>,
}

struct ConversationAttachmentView {
    id: i64,
    display_name: String,
    mime_type: Option<String>,
    mime_category: String,
    size: String,
    file_exists: bool,
    date: String,
    is_image: bool,
}

#[derive(Template)]
#[template(path = "partials/conversation_attachments.html")]
struct ConversationAttachmentsPartialTemplate {
    attachments: Vec<ConversationAttachmentView>,
    conversation_id: i64,
    page: u32,
    has_more: bool,
}

const ATTACHMENTS_PER_PAGE: u32 = 50;

pub async fn conversation_attachments_partial(
    State(state): State<AppState>,
    Query(params): Query<ConversationAttachmentsQuery>,
) -> impl IntoResponse {
    let conversation_id = params.conversation_id;
    let page = params.page.unwrap_or(0);
    let offset = (page * ATTACHMENTS_PER_PAGE) as i64;
    let limit = (ATTACHMENTS_PER_PAGE + 1) as i64;

    let conn = state.db.lock().unwrap();
    let rows = queries::conversation_attachments(&conn, conversation_id, offset, limit)
        .unwrap_or_default();

    let has_more = rows.len() > ATTACHMENTS_PER_PAGE as usize;
    let rows: Vec<_> = rows.into_iter().take(ATTACHMENTS_PER_PAGE as usize).collect();

    let attachments: Vec<ConversationAttachmentView> = rows
        .into_iter()
        .map(|a| ConversationAttachmentView {
            id: a.id,
            display_name: a.display_name().to_string(),
            mime_type: a.mime_type.clone(),
            mime_category: a.mime_category().to_string(),
            size: a.human_size(),
            file_exists: a.file_exists,
            date: a.date_formatted(),
            is_image: a.mime_type.as_deref().map(|m| m.starts_with("image/")).unwrap_or(false),
        })
        .collect();

    let t = ConversationAttachmentsPartialTemplate {
        attachments,
        conversation_id,
        page,
        has_more,
    };
    Html(t.render().unwrap_or_default())
}
