use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse};
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};

use crate::db::queries;
use crate::state::AppState;

fn relative_time(unix: i64) -> String {
    let now = chrono::Utc::now().timestamp();
    let diff = now - unix;
    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        let m = diff / 60;
        format!("{m}m ago")
    } else if diff < 86400 {
        let h = diff / 3600;
        format!("{h}h ago")
    } else if diff < 604800 {
        let d = diff / 86400;
        format!("{d}d ago")
    } else {
        chrono::DateTime::from_timestamp(unix, 0)
            .map(|dt| dt.format("%b %d, %Y").to_string())
            .unwrap_or_default()
    }
}

#[derive(Debug)]
pub struct ConversationRow {
    pub id: i64,
    pub name: String,
    pub last_preview: String,
    pub relative_date: String,
    pub message_count: i64,
}

#[derive(Deserialize)]
pub struct IndexQuery {
    pub filter: Option<String>,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    title: String,
    conversations: Vec<ConversationRow>,
    filter: String,
}

#[derive(Template)]
#[template(path = "partials/conversations.html")]
struct ConversationsPartialTemplate {
    conversations: Vec<ConversationRow>,
}

pub fn build_conversation_rows(state: &AppState, filter: Option<&str>) -> Vec<ConversationRow> {
    let conn = state.db.lock().unwrap();
    let list = queries::conversation_list(&conn, filter).unwrap_or_default();

    list.into_iter()
        .map(|c| {
            let name = c
                .display_name
                .as_ref()
                .filter(|s| !s.is_empty())
                .cloned()
                .or(c.handle.clone())
                .unwrap_or_else(|| "Unknown".to_string());

            let relative_date = c
                .last_message_date
                .map(|ts| relative_time(ts))
                .unwrap_or_default();
            let last_preview = c.last_message_preview.unwrap_or_default();
            ConversationRow {
                id: c.id,
                name,
                last_preview,
                relative_date,
                message_count: c.message_count,
            }
        })
        .collect()
}


pub async fn index(
    headers: HeaderMap,
    Query(params): Query<IndexQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let is_htmx = headers.contains_key("hx-request");
    let filter_str = params.filter.unwrap_or_default();
    let filter_ref = if filter_str.is_empty() {
        None
    } else {
        Some(filter_str.as_str())
    };
    let conversations = build_conversation_rows(&state, filter_ref);

    if is_htmx {
        let t = ConversationsPartialTemplate { conversations };
        Html(t.render().unwrap_or_default())
    } else {
        let t = IndexTemplate {
            title: "iMessage Search".to_string(),
            conversations,
            filter: filter_str,
        };
        Html(t.render().unwrap_or_default())
    }
}

#[derive(Template)]
#[template(path = "conversation.html")]
struct ConversationTemplate {
    title: String,
    conversation_id: i64,
    contact_name: String,
    is_group: bool,
    participants: Vec<String>,
}

pub async fn conversation(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    let info = queries::get_conversation_info(&conn, id);

    let (contact_name, is_group, participants) = match info {
        Ok(info) => {
            let name = info
                .display_name
                .clone()
                .unwrap_or_else(|| {
                    if info.participant_names.is_empty() {
                        "Unknown".to_string()
                    } else {
                        info.participant_names.join(", ")
                    }
                });
            (name, info.is_group, info.participant_names)
        }
        Err(_) => ("Unknown".to_string(), false, vec![]),
    };

    let t = ConversationTemplate {
        title: format!("Conversation with {contact_name}"),
        conversation_id: id,
        contact_name,
        is_group,
        participants,
    };
    Html(t.render().unwrap_or_default())
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
}

#[derive(Template)]
#[template(path = "search.html")]
struct SearchTemplate {
    title: String,
    query: Option<String>,
}

pub async fn search(
    State(_state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> impl IntoResponse {
    let t = SearchTemplate {
        title: "Search".to_string(),
        query: params.q,
    };
    Html(t.render().unwrap_or_default())
}

#[derive(Deserialize)]
pub struct AttachmentsQuery {
    pub filter: Option<String>,
    pub page: Option<u32>,
}

#[derive(Template)]
#[template(path = "attachments.html")]
struct AttachmentsTemplate {
    title: String,
    attachments: Vec<AttachmentView>,
    filter: String,
    page: u32,
    total_pages: u32,
    has_prev: bool,
    has_next: bool,
    total_count: i64,
    image_count: i64,
    video_count: i64,
    audio_count: i64,
    other_count: i64,
}

#[derive(Debug, Serialize)]
struct AttachmentView {
    pub id: i64,
    pub display_name: String,
    pub mime_type: Option<String>,
    pub mime_category: String,
    pub size: String,
    pub file_exists: bool,
    pub conversation_name: Option<String>,
    pub conversation_id: Option<i64>,
    pub date: String,
    pub is_image: bool,
}
pub async fn attachments_page(
    State(state): State<AppState>,
    Query(params): Query<AttachmentsQuery>,
) -> impl IntoResponse {
    const PER_PAGE: i64 = 50;
    
    let page = params.page.unwrap_or(1).max(1) as i64;
    let filter = params.filter.as_deref().filter(|f| !f.is_empty());
    
    let conn = state.db.lock().unwrap();
    
    // Get counts for each category
    let total_count = queries::count_attachments(&conn, None).unwrap_or(0);
    let image_count = queries::count_attachments(&conn, Some("image")).unwrap_or(0);
    let video_count = queries::count_attachments(&conn, Some("video")).unwrap_or(0);
    let audio_count = queries::count_attachments(&conn, Some("audio")).unwrap_or(0);
    let other_count = queries::count_attachments(&conn, Some("other")).unwrap_or(0);
    
    // Get filtered count
    let filtered_count = queries::count_attachments(&conn, filter).unwrap_or(0);
    let total_pages = ((filtered_count + PER_PAGE - 1) / PER_PAGE) as u32;
    let offset = (page - 1) * PER_PAGE;
    
    // Get attachments
    let attachments: Vec<AttachmentView> = queries::list_attachments(&conn, filter, offset, PER_PAGE)
        .unwrap_or_default()
        .into_iter()
        .map(|a| AttachmentView {
            id: a.id,
            display_name: a.display_name().to_string(),
            mime_type: a.mime_type.clone(),
            mime_category: a.mime_category().to_string(),
            size: a.human_size(),
            file_exists: a.file_exists,
            conversation_name: a.conversation_name.clone(),
            conversation_id: a.conversation_id,
            date: a.date_formatted(),
            is_image: a.mime_type.as_deref().map(|m| m.starts_with("image/")).unwrap_or(false),
        })
        .collect();
    
    let t = AttachmentsTemplate {
        title: "Attachments".to_string(),
        attachments,
        filter: filter.unwrap_or("").to_string(),
        page: page as u32,
        total_pages: total_pages.max(1),
        has_prev: page > 1,
        has_next: page < total_pages as i64,
        total_count,
        image_count,
        video_count,
        audio_count,
        other_count,
    };
    
    Html(t.render().unwrap_or_default())
}

#[derive(Debug)]
struct TopConversation {
    rank: usize,
    id: i64,
    name: String,
    count: i64,
}

#[derive(Debug)]
struct TopContact {
    rank: usize,
    name: String,
    handle: String,
    count: i64,
}

#[derive(Debug)]
struct MonthBar {
    label: String,
    count: i64,
    pct: f64,
}

fn format_bytes(bytes: i64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

#[derive(Template)]
#[template(path = "analytics.html")]
struct AnalyticsTemplate {
    title: String,
    total_messages: i64,
    total_conversations: i64,
    total_contacts: i64,
    total_attachments: i64,
    earliest: String,
    latest: String,
    top_conversations: Vec<TopConversation>,
    top_contacts: Vec<TopContact>,
    month_bars: Vec<MonthBar>,
    att_images: i64,
    att_videos: i64,
    att_audio: i64,
    att_other: i64,
    att_total_size: String,
}

pub async fn analytics(State(state): State<AppState>) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();

    let overall = queries::overall_stats(&conn).unwrap_or(queries::OverallStats {
        total_messages: 0,
        total_conversations: 0,
        total_contacts: 0,
        total_attachments: 0,
        earliest_message: None,
        latest_message: None,
    });

    let convos = queries::messages_per_conversation(&conn, 10).unwrap_or_default();
    let top_conversations: Vec<TopConversation> = convos
        .into_iter()
        .enumerate()
        .map(|(i, (id, name, count))| TopConversation {
            rank: i + 1,
            id,
            name,
            count,
        })
        .collect();

    let contacts = queries::top_contacts(&conn, 10).unwrap_or_default();
    let top_contacts: Vec<TopContact> = contacts
        .into_iter()
        .enumerate()
        .map(|(i, (name, handle, count))| TopContact {
            rank: i + 1,
            name,
            handle,
            count,
        })
        .collect();

    let time_data = queries::messages_over_time(&conn, "month").unwrap_or_default();
    let last_12: Vec<&(String, i64)> = time_data.iter().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect();
    let max_count = last_12.iter().map(|(_, c)| *c).max().unwrap_or(1);
    let month_bars: Vec<MonthBar> = last_12
        .into_iter()
        .map(|(label, count)| MonthBar {
            label: label.clone(),
            count: *count,
            pct: (*count as f64 / max_count as f64) * 100.0,
        })
        .collect();

    let att = queries::attachment_stats(&conn).unwrap_or(queries::AttachmentStats {
        total: 0,
        images: 0,
        videos: 0,
        audio: 0,
        other: 0,
        total_bytes: 0,
    });

    let t = AnalyticsTemplate {
        title: "Analytics".to_string(),
        total_messages: overall.total_messages,
        total_conversations: overall.total_conversations,
        total_contacts: overall.total_contacts,
        total_attachments: overall.total_attachments,
        earliest: overall.earliest_message.unwrap_or_else(|| "N/A".to_string()),
        latest: overall.latest_message.unwrap_or_else(|| "N/A".to_string()),
        top_conversations,
        top_contacts,
        month_bars,
        att_images: att.images,
        att_videos: att.videos,
        att_audio: att.audio,
        att_other: att.other,
        att_total_size: format_bytes(att.total_bytes),
    };
    Html(t.render().unwrap_or_default())
}
