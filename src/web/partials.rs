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
    pub focus: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ContributionDay {
    pub date: String,
    pub level: u8,
    pub sent: i64,
    pub received: i64,
}

pub fn format_duration(seconds: f64) -> String {
    let s = seconds.round() as i64;
    if s < 60 {
        "< 1m".to_string()
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else if s < 86400 {
        let h = s / 3600;
        let m = (s % 3600) / 60;
        if m == 0 {
            format!("{}h", h)
        } else {
            format!("{}h {}m", h, m)
        }
    } else {
        let d = s / 86400;
        let h = (s % 86400) / 3600;
        if h == 0 {
            format!("{}d", d)
        } else {
            format!("{}d {}h", d, h)
        }
    }
}

pub fn build_contribution_graph(
    conn: &rusqlite::Connection,
    conversation_id: i64,
    is_group: bool,
) -> Vec<ContributionDay> {
    use std::collections::HashMap;
    let rows = queries::get_mutual_interaction_days(conn, conversation_id, 90).unwrap_or_default();
    let day_map: HashMap<String, &queries::MutualInteractionDay> =
        rows.iter().map(|d| (d.date.clone(), d)).collect();

    let is_active = |d: &&queries::MutualInteractionDay| -> bool {
        if is_group {
            d.sent + d.received > 0
        } else {
            d.sent > 0 && d.received > 0
        }
    };

    let max_total = day_map
        .values()
        .filter(|d| is_active(d))
        .map(|d| d.sent + d.received)
        .max()
        .unwrap_or(1) as f64;

    let today = chrono::Utc::now().date_naive();
    let mut result = Vec::with_capacity(90);
    for i in (0..90).rev() {
        let date = today - chrono::Duration::days(i);
        let date_str = date.format("%Y-%m-%d").to_string();
        let date_display = date.format("%b %-d, %Y").to_string();
        let (level, sent, received) = match day_map.get(&date_str) {
            Some(d) if is_active(d) => {
                let ratio = (d.sent + d.received) as f64 / max_total;
                let l = if ratio > 0.75 {
                    4
                } else if ratio > 0.50 {
                    3
                } else if ratio > 0.25 {
                    2
                } else {
                    1
                };
                (l, d.sent, d.received)
            }
            Some(d) => (0, d.sent, d.received),
            None => (0, 0, 0),
        };
        result.push(ContributionDay {
            date: date_display,
            level,
            sent,
            received,
        });
    }
    result
}

#[derive(Debug, Clone)]
pub struct HourlyStatView {
    pub hour: u8,
    pub label: String,
    pub count: i64,
    pub pct: f64,
    pub is_peak: bool,
}

pub fn build_hourly_stat_views(
    conn: &rusqlite::Connection,
    conversation_id: i64,
) -> Vec<HourlyStatView> {
    let raw = queries::get_hourly_message_stats(conn, conversation_id).unwrap_or_default();
    let max_count = raw.iter().map(|h| h.count).max().unwrap_or(0);
    raw.into_iter()
        .map(|h| {
            let label = match h.hour {
                0 => "12a".to_string(),
                12 => "12p".to_string(),
                h if h < 12 => format!("{}a", h),
                h => format!("{}p", h - 12),
            };
            HourlyStatView {
                hour: h.hour,
                label,
                count: h.count,
                pct: h.pct,
                is_peak: h.count == max_count && max_count > 0,
            }
        })
        .collect()
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
    avg_their_response: Option<String>,
    avg_my_response: Option<String>,
    avg_time_between: Option<String>,
    focus_message_id: Option<i64>,
    group_participant_stats: Vec<queries::GroupParticipantStat>,
    hourly_stats: Vec<HourlyStatView>,
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
    let contribution_days = build_contribution_graph(&conn, id, is_group);

    let (avg_their_response, avg_my_response, avg_time_between) = if is_group {
        let avg = queries::get_avg_time_between_messages(&conn, id)
            .ok()
            .flatten()
            .map(format_duration);
        (None, None, avg)
    } else {
        let times = queries::get_avg_response_times(&conn, id).ok();
        let their = times
            .as_ref()
            .and_then(|t| t.avg_their_response)
            .map(format_duration);
        let mine = times
            .as_ref()
            .and_then(|t| t.avg_my_response)
            .map(format_duration);
        (their, mine, None)
    };

    let (group_participant_stats, hourly_stats) = if is_group {
        let stats = queries::get_group_participant_stats(&conn, id).unwrap_or_default();
        (stats, vec![])
    } else {
        let hourly = build_hourly_stat_views(&conn, id);
        (vec![], hourly)
    };

    let t = ConversationPanelTemplate {
        conversation_id: id,
        contact_name,
        is_group,
        participants,
        attachment_count,
        has_photo,
        contribution_days,
        avg_their_response,
        avg_my_response,
        avg_time_between,
        focus_message_id: params.focus,
        group_participant_stats,
        hourly_stats,
    };
    Html(t.render().unwrap_or_default())
}

#[derive(Deserialize)]
pub struct MessagesQuery {
    pub conversation_id: Option<i64>,
    pub page: Option<u32>,
    pub focus: Option<i64>,
    pub before: Option<i64>,
    pub after: Option<i64>,
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
    sender_id: Option<i64>,
    has_sender_photo: bool,
}

struct MessageGroup {
    is_from_me: bool,
    date_separator: Option<String>,
    messages: Vec<MessageView>,
    sender_id: Option<i64>,
    has_sender_photo: bool,
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
    focus_id: Option<i64>,
    has_newer: bool,
    first_message_id: Option<i64>,
    last_message_id: Option<i64>,
}

const MESSAGES_PER_PAGE: u32 = 50;

pub async fn messages_partial(
    State(state): State<AppState>,
    Query(params): Query<MessagesQuery>,
) -> impl IntoResponse {
    let conversation_id = params.conversation_id.unwrap_or(0);

    let conn = state.db.lock().unwrap();
    let is_group: bool = conn
        .query_row(
            "SELECT is_group FROM conversations WHERE id = ?1",
            [conversation_id],
            |r| r.get(0),
        )
        .unwrap_or(false);

    // Determine which mode we're in
    let (raw_messages, has_more, has_newer, page, focus_id) = if let Some(focus) = params.focus {
        // Focus mode: load messages around the target
        let result = queries::get_messages_around(&conn, conversation_id, focus, 25)
            .unwrap_or_else(|_| queries::MessagesAroundResult {
                messages: Vec::new(),
                has_older: false,
                has_newer: false,
            });
        (
            result.messages,
            result.has_older,
            result.has_newer,
            0u32,
            Some(focus),
        )
    } else if let Some(before_id) = params.before {
        // Before mode: cursor-based load older
        let rows =
            queries::get_messages_before(&conn, conversation_id, before_id, MESSAGES_PER_PAGE + 1)
                .unwrap_or_default();
        let has_more = rows.len() > MESSAGES_PER_PAGE as usize;
        let rows: Vec<_> = rows.into_iter().take(MESSAGES_PER_PAGE as usize).collect();
        (rows, has_more, false, 0u32, None)
    } else if let Some(after_id) = params.after {
        // After mode: cursor-based load newer
        let rows =
            queries::get_messages_after(&conn, conversation_id, after_id, MESSAGES_PER_PAGE + 1)
                .unwrap_or_default();
        let has_newer = rows.len() > MESSAGES_PER_PAGE as usize;
        let rows: Vec<_> = rows.into_iter().take(MESSAGES_PER_PAGE as usize).collect();
        (rows, false, has_newer, 0u32, None)
    } else {
        // Default page mode
        let page = params.page.unwrap_or(0);
        let rows = queries::get_messages(&conn, conversation_id, page, MESSAGES_PER_PAGE + 1)
            .unwrap_or_default();
        let has_more = rows.len() > MESSAGES_PER_PAGE as usize;
        let rows: Vec<_> = rows.into_iter().take(MESSAGES_PER_PAGE as usize).collect();
        (rows, has_more, false, page, None)
    };

    let message_ids: Vec<i64> = raw_messages
        .iter()
        .filter(|m| m.has_attachments)
        .map(|m| m.id)
        .collect();
    let mut att_map = queries::get_message_attachments(&conn, &message_ids).unwrap_or_default();

    let mut messages: Vec<MessageView> = raw_messages
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
                sender_id: m.sender_id,
                has_sender_photo: m.has_sender_photo,
            }
        })
        .collect();

    // For default page mode and before mode, reverse to chronological (query is DESC)
    if params.focus.is_none() && params.after.is_none() {
        messages.reverse();
    }

    let first_message_id = messages.first().map(|m| m.id);
    let last_message_id = messages.last().map(|m| m.id);

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
                sender_id: msg.sender_id,
                has_sender_photo: msg.has_sender_photo,
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
        focus_id,
        has_newer,
        first_message_id,
        last_message_id,
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

#[derive(Deserialize)]
pub struct UnifiedSearchQuery {
    pub q: Option<String>,
    pub page: Option<u32>,
}

struct UnifiedConversationHit {
    id: i64,
    name: String,
    is_group: bool,
    has_photo: bool,
}

struct UnifiedMessageHit {
    id: i64,
    sender_label: String,
    conversation_id: i64,
    conversation_label: String,
    date_formatted: String,
    snippet: String,
}

#[derive(Template)]
#[template(path = "partials/unified_search.html")]
struct UnifiedSearchTemplate {
    query: String,
    conversations: Vec<UnifiedConversationHit>,
    messages: Vec<UnifiedMessageHit>,
    total_message_count: usize,
    has_more: bool,
    next_page: u32,
}

#[derive(Template)]
#[template(path = "partials/unified_search_more.html")]
struct UnifiedSearchMoreTemplate {
    query: String,
    messages: Vec<UnifiedMessageHit>,
    has_more: bool,
    next_page: u32,
}

const UNIFIED_SEARCH_PAGE_SIZE: usize = 20;

pub async fn unified_search_partial(
    State(state): State<AppState>,
    Query(params): Query<UnifiedSearchQuery>,
) -> impl IntoResponse {
    let query = params.q.unwrap_or_default();
    let page = params.page.unwrap_or(0);

    if query.trim().is_empty() {
        let conversations = super::pages::build_conversation_rows(&state, None);
        let t = ConversationsPartialTemplate { conversations };
        return Html(t.render().unwrap_or_default());
    }

    let conn = state.db.lock().unwrap();

    let conversation_hits: Vec<UnifiedConversationHit> = if page == 0 {
        let list = queries::conversation_list(&conn, Some(query.trim())).unwrap_or_default();
        list.into_iter()
            .take(20)
            .map(|c| {
                let name = c
                    .display_name
                    .as_ref()
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .or(c.handle.clone())
                    .unwrap_or_else(|| "Unknown".to_string());
                UnifiedConversationHit {
                    id: c.id,
                    name,
                    is_group: c.is_group,
                    has_photo: c.has_photo,
                }
            })
            .collect()
    } else {
        Vec::new()
    };

    let offset = page as usize * UNIFIED_SEARCH_PAGE_SIZE;
    let results =
        search::search(&conn, &query, UNIFIED_SEARCH_PAGE_SIZE, offset).unwrap_or_default();
    let total_message_count = search::search_count(&conn, &query).unwrap_or(0);

    let messages: Vec<UnifiedMessageHit> = results
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
            UnifiedMessageHit {
                id: r.id,
                sender_label,
                conversation_id: r.conversation_id,
                conversation_label,
                date_formatted: DateTime::from_timestamp(r.date_unix, 0)
                    .map(|dt| dt.format("%b %d, %Y").to_string())
                    .unwrap_or_else(|| "Unknown date".to_string()),
                snippet,
            }
        })
        .collect();

    let fetched = offset + messages.len();
    let has_more = fetched < total_message_count;

    if page > 0 {
        let t = UnifiedSearchMoreTemplate {
            query,
            messages,
            has_more,
            next_page: page + 1,
        };
        return Html(t.render().unwrap_or_default());
    }

    let t = UnifiedSearchTemplate {
        query,
        conversations: conversation_hits,
        messages,
        total_message_count,
        has_more,
        next_page: page + 1,
    };
    Html(t.render().unwrap_or_default())
}

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
    let rows: Vec<_> = rows
        .into_iter()
        .take(ATTACHMENTS_PER_PAGE as usize)
        .collect();

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
            is_image: a
                .mime_type
                .as_deref()
                .map(|m| m.starts_with("image/"))
                .unwrap_or(false),
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
