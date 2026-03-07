use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::{Html, IntoResponse};
use serde::{Deserialize, Serialize};

use crate::db::queries;
use crate::state::AppState;

use super::format::{
    display_initial, format_contact_label, format_contact_value, format_conversation_name,
};
use super::partials::{
    ContributionDay, GroupParticipantStatView, GroupReactionHighlightView, HourlyStatView,
};

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
    pub initial: String,
    pub name: String,
    pub last_preview: String,
    pub relative_date: String,
    pub message_count: i64,
    pub is_group: bool,
    pub has_photo: bool,
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
            let name = format_contact_label(c.display_name.as_deref(), c.handle.as_deref());

            let relative_date = c.last_message_date.map(relative_time).unwrap_or_default();
            let last_preview = c.last_message_preview.unwrap_or_default();
            ConversationRow {
                id: c.id,
                initial: display_initial(&name),
                name,
                last_preview,
                relative_date,
                message_count: c.message_count,
                is_group: c.is_group,
                has_photo: c.has_photo,
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
    contact_initial: String,
    contact_name: String,
    is_group: bool,
    primary_contact_id: Option<i64>,
    participants: Vec<String>,
    attachment_count: i64,
    has_photo: bool,
    contribution_days: Vec<ContributionDay>,
    avg_their_response: Option<String>,
    avg_my_response: Option<String>,
    avg_time_between: Option<String>,
    conversation_started_at: Option<String>,
    conversation_started_ago: Option<String>,
    focus_message_id: Option<i64>,
    group_participant_stats: Vec<GroupParticipantStatView>,
    reaction_highlights: Vec<GroupReactionHighlightView>,
    hourly_stats: Vec<HourlyStatView>,
}

#[derive(Deserialize)]
pub struct ConversationQuery {
    pub focus: Option<i64>,
}

pub async fn conversation(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<ConversationQuery>,
) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    let info = queries::get_conversation_info(&conn, id);
    let primary_contact_id =
        queries::get_primary_contact_id_for_conversation(&conn, id).unwrap_or_default();

    let (contact_name, is_group, participants, has_photo) = match info {
        Ok(info) => {
            let name =
                format_conversation_name(info.display_name.as_deref(), &info.participant_names);
            let participants = super::format::format_contact_list(&info.participant_names);
            (name, info.is_group, participants, info.has_photo)
        }
        Err(_) => ("Unknown".to_string(), false, vec![], false),
    };

    let attachment_count = queries::count_conversation_attachments(&conn, id).unwrap_or(0);
    let contribution_days = super::partials::build_contribution_graph(&conn, id, is_group);
    let conversation_started_unix =
        queries::get_conversation_first_message_unix(&conn, id).unwrap_or_default();

    let (avg_their_response, avg_my_response, avg_time_between) = if is_group {
        let avg = queries::get_avg_time_between_messages(&conn, id)
            .ok()
            .flatten()
            .map(super::partials::format_duration);
        (None, None, avg)
    } else {
        let times = queries::get_avg_response_times(&conn, id).ok();
        let their = times
            .as_ref()
            .and_then(|t| t.avg_their_response)
            .map(super::partials::format_duration);
        let mine = times
            .as_ref()
            .and_then(|t| t.avg_my_response)
            .map(super::partials::format_duration);
        (their, mine, None)
    };

    let (group_participant_stats, reaction_highlights, hourly_stats) = if is_group {
        let stats = super::partials::build_group_participant_stat_views(&conn, id);
        let reaction_highlights = super::partials::build_group_reaction_highlights(&conn, id);
        (stats, reaction_highlights, vec![])
    } else {
        let hourly = super::partials::build_hourly_stat_views(&conn, id);
        (vec![], vec![], hourly)
    };

    let t = ConversationTemplate {
        title: format!("Conversation with {contact_name}"),
        conversation_id: id,
        contact_initial: display_initial(&contact_name),
        contact_name,
        is_group,
        primary_contact_id,
        participants,
        attachment_count,
        has_photo,
        contribution_days,
        avg_their_response,
        avg_my_response,
        avg_time_between,
        conversation_started_at: conversation_started_unix
            .and_then(super::partials::format_conversation_start),
        conversation_started_ago: conversation_started_unix
            .and_then(super::partials::format_conversation_start_elapsed),
        focus_message_id: params.focus,
        group_participant_stats,
        reaction_highlights,
        hourly_stats,
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
    pub sync: Option<String>,
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
    sync_filter: String,
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
    pub is_video: bool,
    pub is_audio: bool,
    pub has_preview: bool,
    pub sync_status: String,
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
    let attachments: Vec<AttachmentView> =
        queries::list_attachments(&conn, filter, offset, PER_PAGE)
            .unwrap_or_default()
            .into_iter()
            .map(|a| {
                let is_image = a
                    .mime_type
                    .as_deref()
                    .map(|m| m.starts_with("image/"))
                    .unwrap_or(false);
                let is_video = a
                    .mime_type
                    .as_deref()
                    .map(|m| m.starts_with("video/"))
                    .unwrap_or(false);
                let is_audio = a
                    .mime_type
                    .as_deref()
                    .map(|m| m.starts_with("audio/"))
                    .unwrap_or(false);
                let has_preview = is_image || is_video;
                AttachmentView {
                    id: a.id,
                    display_name: a.display_name().to_string(),
                    mime_type: a.mime_type.clone(),
                    mime_category: a.mime_category().to_string(),
                    size: a.human_size(),
                    file_exists: a.file_exists,
                    conversation_name: a.conversation_name.clone(),
                    conversation_id: a.conversation_id,
                    date: a.date_formatted(),
                    is_image,
                    is_video,
                    is_audio,
                    has_preview,
                    sync_status: match a.ck_sync_state {
                        0 => "local".to_string(),
                        1 => "icloud".to_string(),
                        2 => "pending".to_string(),
                        _ => "error".to_string(),
                    },
                }
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
        sync_filter: params.sync.clone().unwrap_or_default(),
    };

    Html(t.render().unwrap_or_default())
}

#[derive(Debug)]
struct ContactInsightDayBar {
    label: String,
    count: i64,
    pct: f64,
    is_peak: bool,
}

#[derive(Template)]
#[template(path = "contact_insights.html")]
struct ContactInsightsTemplate {
    title: String,
    contact_id: i64,
    contact_name: String,
    contact_initial: String,
    contact_handle: String,
    has_photo: bool,
    has_conversation: bool,
    conversation_link: String,
    sent_count: i64,
    received_count: i64,
    sent_ratio: String,
    received_ratio: String,
    first_message: String,
    last_message: String,
    longest_streak: i64,
    streak_label: String,
    my_starts: i64,
    their_starts: i64,
    my_initiative_pct: f64,
    their_initiative_pct: f64,
    my_initiative_label: String,
    their_initiative_label: String,
    initiative_summary: String,
    day_bars: Vec<ContactInsightDayBar>,
    my_reactions: i64,
    their_reactions: i64,
    trend_label: String,
    trend_summary: String,
    trend_recent: i64,
    trend_prior: i64,
}

fn format_ratio(count: i64, total: i64) -> String {
    if total <= 0 {
        "0%".to_string()
    } else {
        format!("{:.0}%", (count as f64 / total as f64) * 100.0)
    }
}

fn build_initiative_summary(my_starts: i64, their_starts: i64) -> String {
    match my_starts.cmp(&their_starts) {
        std::cmp::Ordering::Greater => "You usually reopen the conversation first.".to_string(),
        std::cmp::Ordering::Less => "They usually reopen the conversation first.".to_string(),
        std::cmp::Ordering::Equal if my_starts == 0 => "No restart moments yet.".to_string(),
        std::cmp::Ordering::Equal => "You both start conversations equally often.".to_string(),
    }
}

fn build_trend(trend: &queries::ContactTrendStats) -> (String, String) {
    match trend.recent_count.cmp(&trend.prior_count) {
        std::cmp::Ordering::Greater => (
            "Increasing".to_string(),
            format!(
                "{} messages in the last 90 days vs {} in the previous 90.",
                trend.recent_count, trend.prior_count
            ),
        ),
        std::cmp::Ordering::Less => (
            "Declining".to_string(),
            format!(
                "{} messages in the last 90 days vs {} in the previous 90.",
                trend.recent_count, trend.prior_count
            ),
        ),
        std::cmp::Ordering::Equal => (
            "Stable".to_string(),
            format!(
                "{} messages in both the last 90 days and the prior 90.",
                trend.recent_count
            ),
        ),
    }
}

pub async fn contact_insights(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();

    let contact = queries::get_contact_basic_info(&conn, id)
        .unwrap_or_default()
        .unwrap_or(queries::ContactBasicInfo {
            id,
            name: "Unknown".to_string(),
            handle: "Unknown".to_string(),
            has_photo: false,
        });
    let contact_name = format_contact_value(&contact.name);
    let contact_handle = format_contact_value(&contact.handle);
    let conversation_id = queries::get_contact_conversation_id(&conn, id).unwrap_or_default();

    let (
        sent_count,
        received_count,
        first_message,
        last_message,
        longest_streak,
        my_starts,
        their_starts,
        day_bars,
        my_reactions,
        their_reactions,
        trend_recent,
        trend_prior,
        trend_label,
        trend_summary,
    ) = if let Some(conversation_id) = conversation_id {
        let message_counts =
            queries::get_contact_message_counts(&conn, conversation_id).unwrap_or_default();
        let date_range =
            queries::get_contact_first_last_dates(&conn, conversation_id).unwrap_or_default();
        let longest_streak =
            queries::get_contact_longest_streak(&conn, conversation_id).unwrap_or(0);
        let initiative =
            queries::get_contact_initiative_stats(&conn, conversation_id).unwrap_or_default();
        let weekday_stats =
            queries::get_contact_day_of_week_stats(&conn, conversation_id).unwrap_or_default();
        let max_day_count = weekday_stats.iter().map(|day| day.count).max().unwrap_or(0);
        let day_bars = weekday_stats
            .into_iter()
            .map(|day| ContactInsightDayBar {
                label: day.label,
                count: day.count,
                pct: day.pct,
                is_peak: max_day_count > 0 && day.count == max_day_count,
            })
            .collect();
        let reactions =
            queries::get_contact_reaction_counts(&conn, conversation_id).unwrap_or_default();
        let trend = queries::get_contact_trend_stats(&conn, conversation_id).unwrap_or_default();
        let (trend_label, trend_summary) = build_trend(&trend);

        (
            message_counts.sent,
            message_counts.received,
            date_range
                .first_message
                .unwrap_or_else(|| "N/A".to_string()),
            date_range.last_message.unwrap_or_else(|| "N/A".to_string()),
            longest_streak,
            initiative.my_starts,
            initiative.their_starts,
            day_bars,
            reactions.my_reactions,
            reactions.their_reactions,
            trend.recent_count,
            trend.prior_count,
            trend_label,
            trend_summary,
        )
    } else {
        (
            0,
            0,
            "N/A".to_string(),
            "N/A".to_string(),
            0,
            0,
            0,
            vec![
                ContactInsightDayBar {
                    label: "Mon".to_string(),
                    count: 0,
                    pct: 0.0,
                    is_peak: false,
                },
                ContactInsightDayBar {
                    label: "Tue".to_string(),
                    count: 0,
                    pct: 0.0,
                    is_peak: false,
                },
                ContactInsightDayBar {
                    label: "Wed".to_string(),
                    count: 0,
                    pct: 0.0,
                    is_peak: false,
                },
                ContactInsightDayBar {
                    label: "Thu".to_string(),
                    count: 0,
                    pct: 0.0,
                    is_peak: false,
                },
                ContactInsightDayBar {
                    label: "Fri".to_string(),
                    count: 0,
                    pct: 0.0,
                    is_peak: false,
                },
                ContactInsightDayBar {
                    label: "Sat".to_string(),
                    count: 0,
                    pct: 0.0,
                    is_peak: false,
                },
                ContactInsightDayBar {
                    label: "Sun".to_string(),
                    count: 0,
                    pct: 0.0,
                    is_peak: false,
                },
            ],
            0,
            0,
            0,
            0,
            "Stable".to_string(),
            "No one-on-one conversation data available yet.".to_string(),
        )
    };

    let total_messages = sent_count + received_count;
    let total_starts = my_starts + their_starts;
    let contact_initial = display_initial(&contact_name);
    let t = ContactInsightsTemplate {
        title: format!("{contact_name} Insights"),
        contact_id: contact.id,
        contact_name,
        contact_initial,
        contact_handle,
        has_photo: contact.has_photo,
        has_conversation: conversation_id.is_some(),
        conversation_link: conversation_id
            .map(|conversation_id| format!("/conversations/{conversation_id}"))
            .unwrap_or_default(),
        sent_count,
        received_count,
        sent_ratio: format_ratio(sent_count, total_messages),
        received_ratio: format_ratio(received_count, total_messages),
        first_message,
        last_message,
        longest_streak,
        streak_label: if longest_streak == 1 {
            "day".to_string()
        } else {
            "days".to_string()
        },
        my_starts,
        their_starts,
        my_initiative_pct: if total_starts > 0 {
            (my_starts as f64 / total_starts as f64) * 100.0
        } else {
            50.0
        },
        their_initiative_pct: if total_starts > 0 {
            (their_starts as f64 / total_starts as f64) * 100.0
        } else {
            50.0
        },
        my_initiative_label: format_ratio(my_starts, total_starts),
        their_initiative_label: format_ratio(their_starts, total_starts),
        initiative_summary: build_initiative_summary(my_starts, their_starts),
        day_bars,
        my_reactions,
        their_reactions,
        trend_label,
        trend_summary,
        trend_recent,
        trend_prior,
    };

    Html(t.render().unwrap_or_default())
}

pub async fn conversation_photo(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let photo = {
        let conn = state.db.lock().unwrap();
        queries::get_conversation_photo(&conn, id)
    };

    match photo {
        Ok(Some(queries::ConversationPhoto::ContactBlob(bytes))) => {
            let content_type = if bytes.starts_with(b"\x89PNG") {
                "image/png"
            } else {
                "image/jpeg"
            };
            (
                axum::http::StatusCode::OK,
                [
                    (axum::http::header::CONTENT_TYPE, content_type),
                    (axum::http::header::CACHE_CONTROL, "public, max-age=86400"),
                ],
                bytes,
            )
                .into_response()
        }
        Ok(Some(queries::ConversationPhoto::GroupFilePath(path))) => {
            match tokio::fs::read(&path).await {
                Ok(bytes) => {
                    let content_type = if path.ends_with(".png") {
                        "image/png"
                    } else {
                        "image/jpeg"
                    };
                    (
                        axum::http::StatusCode::OK,
                        [
                            (axum::http::header::CONTENT_TYPE, content_type),
                            (axum::http::header::CACHE_CONTROL, "public, max-age=86400"),
                        ],
                        bytes,
                    )
                        .into_response()
                }
                Err(_) => axum::http::StatusCode::NOT_FOUND.into_response(),
            }
        }
        _ => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}

pub async fn contact_photo(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let photo = {
        let conn = state.db.lock().unwrap();
        queries::get_contact_photo(&conn, id)
    };

    match photo {
        Ok(Some(bytes)) => {
            let content_type = if bytes.starts_with(b"\x89PNG") {
                "image/png"
            } else {
                "image/jpeg"
            };
            (
                axum::http::StatusCode::OK,
                [
                    (axum::http::header::CONTENT_TYPE, content_type),
                    (axum::http::header::CACHE_CONTROL, "public, max-age=86400"),
                ],
                bytes,
            )
                .into_response()
        }
        _ => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}
