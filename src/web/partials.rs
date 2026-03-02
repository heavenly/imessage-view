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

struct AttachmentView {
    id: i64,
    mime_type: Option<String>,
    transfer_name: Option<String>,
    total_bytes: Option<i64>,
}

#[derive(Template)]
#[template(path = "partials/messages.html")]
struct MessagesPartialTemplate {
    messages: Vec<MessageView>,
    conversation_id: i64,
    page: u32,
    has_more: bool,
}

const MESSAGES_PER_PAGE: u32 = 50;

pub async fn messages_partial(
    State(state): State<AppState>,
    Query(params): Query<MessagesQuery>,
) -> impl IntoResponse {
    let conversation_id = params.conversation_id.unwrap_or(0);
    let page = params.page.unwrap_or(0);

    let conn = state.db.lock().unwrap();
    let rows = queries::get_messages(&conn, conversation_id, page, MESSAGES_PER_PAGE + 1)
        .unwrap_or_default();

    let has_more = rows.len() > MESSAGES_PER_PAGE as usize;
    let rows: Vec<_> = rows.into_iter().take(MESSAGES_PER_PAGE as usize).collect();

    let message_ids: Vec<i64> = rows.iter().filter(|m| m.has_attachments).map(|m| m.id).collect();
    let mut att_map = queries::get_message_attachments(&conn, &message_ids).unwrap_or_default();

    let messages: Vec<MessageView> = rows
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

    let t = MessagesPartialTemplate {
        messages,
        conversation_id,
        page,
        has_more,
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
