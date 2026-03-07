use askama::Template;
use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse};
use chrono::{DateTime, Datelike, Local, NaiveDate};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::db::queries;
use crate::search;
use crate::state::AppState;

use super::format::{
    display_initial, format_contact_label, format_contact_list, format_contact_value,
    format_conversation_name, format_group_participant_summary,
};
use super::pages::ConversationRow;

fn canonical_conversation_id(conn: &rusqlite::Connection, conversation_id: i64) -> i64 {
    queries::resolve_canonical_conversation_id(conn, conversation_id)
        .ok()
        .flatten()
        .unwrap_or(conversation_id)
}

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

#[derive(Debug, Clone)]
pub struct ContributionWeek {
    pub month_label: Option<String>,
    pub days: Vec<Option<ContributionDay>>,
}

#[derive(Debug, Clone)]
pub struct ContributionGraph {
    pub weekday_labels: Vec<String>,
    pub weeks: Vec<ContributionWeek>,
    pub week_count: usize,
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

pub fn format_conversation_start(unix: i64) -> Option<String> {
    DateTime::from_timestamp(unix, 0)
        .map(|dt| dt.with_timezone(&Local).format("%b %-d, %Y").to_string())
}

fn calendar_elapsed_since(start_date: NaiveDate, end_date: NaiveDate) -> String {
    if start_date >= end_date {
        return "today".to_string();
    }

    let mut years = end_date.year() - start_date.year();
    let mut months = end_date.month() as i32 - start_date.month() as i32;
    let mut days = end_date.day() as i32 - start_date.day() as i32;

    if days < 0 {
        months -= 1;
        let first_of_month = end_date.with_day(1).unwrap_or(end_date);
        let days_in_previous_month = first_of_month
            .pred_opt()
            .map(|date| date.day() as i32)
            .unwrap_or(30);
        days += days_in_previous_month;
    }

    if months < 0 {
        years -= 1;
        months += 12;
    }

    let mut parts = Vec::new();
    if years > 0 {
        parts.push(format!(
            "{years} {}",
            if years == 1 { "year" } else { "years" }
        ));
    }
    if months > 0 {
        parts.push(format!(
            "{months} {}",
            if months == 1 { "month" } else { "months" }
        ));
    }
    if days > 0 || parts.is_empty() {
        parts.push(format!("{days} {}", if days == 1 { "day" } else { "days" }));
    }

    format!("{} ago", parts.join(", "))
}

pub fn format_conversation_start_elapsed(unix: i64) -> Option<String> {
    DateTime::from_timestamp(unix, 0).map(|dt| {
        let start_date = dt.with_timezone(&Local).date_naive();
        let today = Local::now().date_naive();
        calendar_elapsed_since(start_date, today)
    })
}

pub fn build_contribution_graph(
    conn: &rusqlite::Connection,
    conversation_id: i64,
    is_group: bool,
) -> ContributionGraph {
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
    let oldest_date = today - chrono::Duration::days(89);
    let grid_start =
        oldest_date - chrono::Duration::days(oldest_date.weekday().num_days_from_monday() as i64);
    let grid_end =
        today + chrono::Duration::days((6 - today.weekday().num_days_from_monday()) as i64);

    let mut current = grid_start;
    let mut weeks = Vec::new();
    let mut previous_month: Option<u32> = None;

    while current <= grid_end {
        let mut days = Vec::with_capacity(7);
        let mut first_in_range_month: Option<u32> = None;
        let mut first_in_range_label: Option<String> = None;

        for _ in 0..7 {
            if current >= oldest_date && current <= today {
                if first_in_range_month.is_none() {
                    first_in_range_month = Some(current.month());
                    first_in_range_label = Some(current.format("%b").to_string());
                }

                let date_str = current.format("%Y-%m-%d").to_string();
                let date_display = current.format("%b %-d, %Y").to_string();
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

                days.push(Some(ContributionDay {
                    date: date_display,
                    level,
                    sent,
                    received,
                }));
            } else {
                days.push(None);
            }

            current += chrono::Duration::days(1);
        }

        let month_label = match first_in_range_month {
            Some(month) if previous_month != Some(month) => {
                previous_month = Some(month);
                first_in_range_label
            }
            _ => None,
        };

        weeks.push(ContributionWeek { month_label, days });
    }

    ContributionGraph {
        weekday_labels: ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        week_count: weeks.len(),
        weeks,
    }
}

#[derive(Debug, Clone)]
pub struct HourlyStatView {
    pub hour: u8,
    pub label: String,
    pub count: i64,
    pub pct: f64,
    pub is_peak: bool,
}

#[derive(Debug, Clone)]
pub struct GroupReactionHighlightView {
    pub title: String,
    pub glyph: String,
    pub metric: String,
    pub sender_name: String,
    pub preview: String,
    pub message_id: i64,
}

#[derive(Debug, Clone)]
pub struct GroupParticipantStatView {
    pub contact_id: Option<i64>,
    pub name: String,
    pub initial: String,
    pub message_count: i64,
    pub percentage: String,
    pub has_photo: bool,
}

pub fn build_group_participant_stat_views(
    conn: &rusqlite::Connection,
    conversation_id: i64,
) -> Vec<GroupParticipantStatView> {
    queries::get_group_participant_stats(conn, conversation_id)
        .unwrap_or_default()
        .into_iter()
        .map(|participant| {
            let name = format_contact_value(&participant.name);
            GroupParticipantStatView {
                contact_id: participant.contact_id,
                initial: display_initial(&name),
                name,
                message_count: participant.message_count,
                percentage: participant.percentage,
                has_photo: participant.has_photo,
            }
        })
        .collect()
}

fn built_in_reaction_glyph(reaction_type: i64) -> Option<&'static str> {
    match reaction_type {
        2000 => Some("❤️"),
        2001 => Some("👍"),
        2002 => Some("👎"),
        2003 => Some("HaHa"),
        2004 => Some("‼"),
        2005 => Some("?"),
        _ => None,
    }
}

fn built_in_reaction_noun(reaction_type: i64) -> Option<&'static str> {
    match reaction_type {
        2000 => Some("love"),
        2001 => Some("like"),
        2002 => Some("dislike"),
        2003 => Some("laugh"),
        2004 => Some("exclamation"),
        2005 => Some("question"),
        _ => None,
    }
}

fn message_preview(body: Option<&str>, has_attachments: bool) -> String {
    let trimmed = body.unwrap_or_default().trim();
    if !trimmed.is_empty() {
        let mut preview = String::new();
        let mut chars = trimmed.chars();
        for _ in 0..80 {
            if let Some(ch) = chars.next() {
                preview.push(ch);
            } else {
                return preview;
            }
        }
        if chars.next().is_some() {
            preview.push_str("...");
        }
        preview
    } else if has_attachments {
        "Attachment".to_string()
    } else {
        "Message".to_string()
    }
}

fn pluralize(label: &str, count: i64) -> String {
    if count == 1 {
        label.to_string()
    } else {
        format!("{label}s")
    }
}

pub fn build_group_reaction_highlights(
    conn: &rusqlite::Connection,
    conversation_id: i64,
) -> Vec<GroupReactionHighlightView> {
    let rows = match queries::get_group_reaction_highlight_rows(conn, conversation_id) {
        Ok(rows) => rows,
        Err(err) => {
            eprintln!(
                "failed to load group reaction highlights for conversation {conversation_id}: {err}"
            );
            return vec![];
        }
    };

    if rows.is_empty() {
        return vec![];
    }

    #[derive(Clone)]
    struct MessageSummary {
        id: i64,
        body: Option<String>,
        date_unix: i64,
        sender_name: Option<String>,
        has_attachments: bool,
    }

    #[derive(Clone)]
    struct BestHighlight {
        count: i64,
        message: MessageSummary,
    }

    fn update_best(best: &mut Option<BestHighlight>, count: i64, message: &MessageSummary) {
        let candidate = BestHighlight {
            count,
            message: message.clone(),
        };

        let should_replace = match best {
            Some(current) => {
                (
                    candidate.count,
                    candidate.message.date_unix,
                    candidate.message.id,
                ) > (current.count, current.message.date_unix, current.message.id)
            }
            None => true,
        };

        if should_replace {
            *best = Some(candidate);
        }
    }

    fn finalize_effective_message(
        message: &Option<MessageSummary>,
        by_sender: &HashMap<String, i64>,
        reaction_winners: &mut HashMap<i64, BestHighlight>,
        varied_winner: &mut Option<BestHighlight>,
    ) {
        let Some(message) = message else {
            return;
        };

        let mut counts: HashMap<i64, i64> = HashMap::new();
        for reaction_type in by_sender.values().copied() {
            *counts.entry(reaction_type).or_insert(0) += 1;
        }

        if counts.is_empty() {
            return;
        }

        for (&reaction_type, &count) in &counts {
            let should_replace = match reaction_winners.get(&reaction_type) {
                Some(current) => {
                    (count, message.date_unix, message.id)
                        > (current.count, current.message.date_unix, current.message.id)
                }
                None => true,
            };

            if should_replace {
                reaction_winners.insert(
                    reaction_type,
                    BestHighlight {
                        count,
                        message: message.clone(),
                    },
                );
            }
        }

        update_best(varied_winner, counts.len() as i64, message);
    }

    let mut current_message_id = None;
    let mut current_message: Option<MessageSummary> = None;
    let mut by_sender: HashMap<String, i64> = HashMap::new();
    let mut reaction_winners: HashMap<i64, BestHighlight> = HashMap::new();
    let mut varied_winner: Option<BestHighlight> = None;

    for row in rows {
        if current_message_id != Some(row.message_id) {
            finalize_effective_message(
                &current_message,
                &by_sender,
                &mut reaction_winners,
                &mut varied_winner,
            );
            current_message_id = Some(row.message_id);
            current_message = Some(MessageSummary {
                id: row.message_id,
                body: row.message_body.clone(),
                date_unix: row.message_date_unix,
                sender_name: row.message_sender_name.clone(),
                has_attachments: row.message_has_attachments,
            });
            by_sender.clear();
        }

        let sender_key = if row.reaction_is_from_me {
            "me".to_string()
        } else {
            row.reaction_sender_name
                .clone()
                .unwrap_or_else(|| "unknown".to_string())
        };

        if (3000..=3007).contains(&row.reaction_type) {
            by_sender.remove(&sender_key);
        } else if built_in_reaction_glyph(row.reaction_type).is_some() {
            by_sender.insert(sender_key, row.reaction_type);
        }
    }

    finalize_effective_message(
        &current_message,
        &by_sender,
        &mut reaction_winners,
        &mut varied_winner,
    );

    let mut highlights = Vec::new();
    for (reaction_type, title) in [
        (2000, "Most Loved"),
        (2001, "Most Liked"),
        (2002, "Most Disliked"),
        (2003, "Most Laughed At"),
        (2004, "Most Exclaimed"),
        (2005, "Most Questioned"),
    ] {
        if let Some(best) = reaction_winners.get(&reaction_type) {
            let noun = built_in_reaction_noun(reaction_type).unwrap_or("reaction");
            highlights.push(GroupReactionHighlightView {
                title: title.to_string(),
                glyph: built_in_reaction_glyph(reaction_type)
                    .unwrap_or("?")
                    .to_string(),
                metric: format!("{} {}", best.count, pluralize(noun, best.count)),
                sender_name: best
                    .message
                    .sender_name
                    .clone()
                    .map(|name| format_contact_value(&name))
                    .unwrap_or_else(|| "Unknown".to_string()),
                preview: message_preview(
                    best.message.body.as_deref(),
                    best.message.has_attachments,
                ),
                message_id: best.message.id,
            });
        }
    }

    if let Some(best) = varied_winner {
        highlights.push(GroupReactionHighlightView {
            title: "Most Varied Reactions".to_string(),
            glyph: "✨".to_string(),
            metric: format!("{} {}", best.count, pluralize("type", best.count)),
            sender_name: best
                .message
                .sender_name
                .clone()
                .map(|name| format_contact_value(&name))
                .unwrap_or_else(|| "Unknown".to_string()),
            preview: message_preview(best.message.body.as_deref(), best.message.has_attachments),
            message_id: best.message.id,
        });
    }

    highlights
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
    contact_initial: String,
    contact_name: String,
    is_group: bool,
    primary_contact_id: Option<i64>,
    participant_summary: String,
    attachment_count: Option<i64>,
    has_photo: bool,
    focus_message_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ConversationShellData {
    pub conversation_id: i64,
    pub contact_initial: String,
    pub contact_name: String,
    pub is_group: bool,
    pub primary_contact_id: Option<i64>,
    pub participants: Vec<String>,
    pub participant_summary: String,
    pub has_photo: bool,
}

#[derive(Debug, Clone)]
pub struct ConversationInsightsData {
    pub attachment_count: i64,
    pub contribution_graph: ContributionGraph,
    pub avg_their_response: Option<String>,
    pub avg_my_response: Option<String>,
    pub avg_time_between: Option<String>,
    pub conversation_started_at: Option<String>,
    pub conversation_started_ago: Option<String>,
    pub group_participant_stats: Vec<GroupParticipantStatView>,
    pub reaction_highlights: Vec<GroupReactionHighlightView>,
    pub hourly_stats: Vec<HourlyStatView>,
}

#[derive(Template)]
#[template(path = "partials/conversation_insights.html")]
struct ConversationInsightsTemplate {
    conversation_id: i64,
    is_group: bool,
    primary_contact_id: Option<i64>,
    participants: Vec<String>,
    insights: ConversationInsightsData,
}

const CONVERSATION_INSIGHTS_TTL: Duration = Duration::from_secs(45);

pub fn build_conversation_shell(
    conn: &rusqlite::Connection,
    conversation_id: i64,
) -> ConversationShellData {
    let info = queries::get_conversation_info(conn, conversation_id);
    let primary_contact_id =
        queries::get_primary_contact_id_for_conversation(conn, conversation_id).unwrap_or_default();

    let (contact_name, is_group, participants, has_photo) = match info {
        Ok(info) => {
            let name =
                format_conversation_name(info.display_name.as_deref(), &info.participant_names);
            let participants = format_contact_list(&info.participant_names);
            (name, info.is_group, participants, info.has_photo)
        }
        Err(_) => ("Unknown".to_string(), false, vec![], false),
    };

    ConversationShellData {
        conversation_id,
        contact_initial: display_initial(&contact_name),
        contact_name,
        is_group,
        primary_contact_id,
        participant_summary: format_group_participant_summary(&participants),
        participants,
        has_photo,
    }
}

fn build_conversation_insights_data(
    conn: &rusqlite::Connection,
    shell: &ConversationShellData,
) -> ConversationInsightsData {
    let attachment_count =
        queries::count_conversation_attachments(conn, shell.conversation_id).unwrap_or(0);
    let contribution_graph = build_contribution_graph(conn, shell.conversation_id, shell.is_group);
    let conversation_started_unix =
        queries::get_conversation_first_message_unix(conn, shell.conversation_id)
            .unwrap_or_default();

    let (avg_their_response, avg_my_response, avg_time_between) = if shell.is_group {
        let avg = queries::get_avg_time_between_messages(conn, shell.conversation_id)
            .ok()
            .flatten()
            .map(format_duration);
        (None, None, avg)
    } else {
        let times = queries::get_avg_response_times(conn, shell.conversation_id).ok();
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

    let (group_participant_stats, reaction_highlights, hourly_stats) = if shell.is_group {
        let stats = build_group_participant_stat_views(conn, shell.conversation_id);
        let reaction_highlights = build_group_reaction_highlights(conn, shell.conversation_id);
        (stats, reaction_highlights, vec![])
    } else {
        let hourly = build_hourly_stat_views(conn, shell.conversation_id);
        (vec![], vec![], hourly)
    };

    ConversationInsightsData {
        attachment_count,
        contribution_graph,
        avg_their_response,
        avg_my_response,
        avg_time_between,
        conversation_started_at: conversation_started_unix.and_then(format_conversation_start),
        conversation_started_ago: conversation_started_unix
            .and_then(format_conversation_start_elapsed),
        group_participant_stats,
        reaction_highlights,
        hourly_stats,
    }
}

fn get_cached_conversation_insights(
    state: &AppState,
    conversation_id: i64,
) -> Option<ConversationInsightsData> {
    let now = Instant::now();
    state
        .conversation_insights_cache
        .read()
        .unwrap()
        .get(&conversation_id)
        .filter(|entry| entry.expires_at > now)
        .map(|entry| entry.data.clone())
}

fn cache_conversation_insights(
    state: &AppState,
    conversation_id: i64,
    data: &ConversationInsightsData,
) {
    state.conversation_insights_cache.write().unwrap().insert(
        conversation_id,
        crate::state::CachedConversationInsights {
            data: data.clone(),
            expires_at: Instant::now() + CONVERSATION_INSIGHTS_TTL,
        },
    );
}

pub async fn conversation_panel_partial(
    State(state): State<AppState>,
    Query(params): Query<ConversationPanelQuery>,
) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
    let id = canonical_conversation_id(&conn, params.id);
    let shell = build_conversation_shell(&conn, id);

    let t = ConversationPanelTemplate {
        conversation_id: shell.conversation_id,
        contact_initial: shell.contact_initial,
        contact_name: shell.contact_name,
        is_group: shell.is_group,
        primary_contact_id: shell.primary_contact_id,
        participant_summary: shell.participant_summary,
        attachment_count: None,
        has_photo: shell.has_photo,
        focus_message_id: params.focus,
    };
    Html(t.render().unwrap_or_default())
}

pub async fn conversation_insights_partial(
    State(state): State<AppState>,
    Query(params): Query<ConversationPanelQuery>,
) -> impl IntoResponse {
    let id = {
        let conn = state.db.lock().unwrap();
        canonical_conversation_id(&conn, params.id)
    };

    let shell = {
        let conn = state.db.lock().unwrap();
        build_conversation_shell(&conn, id)
    };

    let insights = if let Some(cached) = get_cached_conversation_insights(&state, id) {
        cached
    } else {
        let computed = {
            let conn = state.db.lock().unwrap();
            build_conversation_insights_data(&conn, &shell)
        };
        cache_conversation_insights(&state, id, &computed);
        computed
    };

    let t = ConversationInsightsTemplate {
        conversation_id: id,
        is_group: shell.is_group,
        primary_contact_id: shell.primary_contact_id,
        participants: shell.participants,
        insights,
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
    body_html: Option<String>,
    is_from_me: bool,
    service: Option<String>,
    sender_initial: Option<String>,
    sender_name: Option<String>,
    has_attachments: bool,
    time_formatted: String,
    date_formatted: String,
    attachments: Vec<AttachmentView>,
    reactions: Vec<ReactionView>,
    use_attachment_grid: bool,
    is_sticker_only: bool,
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
    is_sticker: bool,
    is_image: bool,
}

struct ReactionView {
    glyph: String,
    title: String,
    is_textual: bool,
    is_haha: bool,
}

fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn split_trailing_punctuation(url: &str) -> (&str, &str) {
    let mut cut = url.len();
    for (idx, ch) in url.char_indices().rev() {
        if matches!(ch, '.' | ',' | '!' | '?' | ':' | ';' | ')' | ']') {
            cut = idx;
            continue;
        }
        break;
    }
    url.split_at(cut)
}

fn linkify_text(input: &str) -> String {
    let mut html = String::new();
    let mut cursor = 0;

    while let Some(relative_start) = {
        let segment = &input[cursor..];
        match (segment.find("http://"), segment.find("https://")) {
            (Some(http), Some(https)) => Some(http.min(https)),
            (Some(http), None) => Some(http),
            (None, Some(https)) => Some(https),
            (None, None) => None,
        }
    } {
        let start = cursor + relative_start;
        html.push_str(&escape_html(&input[cursor..start]));

        let rest = &input[start..];
        let end = rest
            .find(char::is_whitespace)
            .map_or(input.len(), |idx| start + idx);
        let raw_url = &input[start..end];
        let (clean_url, trailing) = split_trailing_punctuation(raw_url);

        if clean_url.is_empty() {
            html.push_str(&escape_html(raw_url));
        } else {
            let escaped_url = escape_html(clean_url);
            html.push_str("<a href=\"");
            html.push_str(&escaped_url);
            html.push_str("\" target=\"_blank\" rel=\"noreferrer noopener\">");
            html.push_str(&escaped_url);
            html.push_str("</a>");
            html.push_str(&escape_html(trailing));
        }

        cursor = end;
    }

    html.push_str(&escape_html(&input[cursor..]));
    html
}

fn reaction_view(reaction: &queries::MessageReaction) -> Option<ReactionView> {
    let (glyph, title, is_textual) = match reaction.reaction_type {
        2000 => ("❤️", "Loved", false),
        2001 => ("👍", "Liked", false),
        2002 => ("👎", "Disliked", false),
        2003 => ("HaHa", "Laughed", true),
        2004 => ("‼", "Emphasized", false),
        2005 => ("?", "Questioned", true),
        2006 => (
            reaction.reaction_emoji.as_deref().unwrap_or("🙂"),
            "Reacted",
            false,
        ),
        _ => return None,
    };

    Some(ReactionView {
        glyph: glyph.to_string(),
        title: title.to_string(),
        is_textual,
        is_haha: reaction.reaction_type == 2003,
    })
}

fn build_reactions(reactions: Vec<queries::MessageReaction>) -> Vec<ReactionView> {
    let mut by_sender: HashMap<String, ReactionView> = HashMap::new();

    for reaction in reactions {
        let sender_key = if reaction.is_from_me {
            "me".to_string()
        } else {
            reaction
                .sender_name
                .clone()
                .unwrap_or_else(|| "unknown".to_string())
        };

        if (3000..=3007).contains(&reaction.reaction_type) {
            by_sender.remove(&sender_key);
            continue;
        }

        if let Some(view) = reaction_view(&reaction) {
            by_sender.insert(sender_key, view);
        }
    }

    let mut resolved: Vec<ReactionView> = by_sender.into_values().collect();
    resolved.sort_by(|left, right| left.title.cmp(&right.title));
    resolved
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
    let conn = state.db.lock().unwrap();
    let conversation_id = canonical_conversation_id(&conn, params.conversation_id.unwrap_or(0));
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
    let message_guids: Vec<String> = raw_messages.iter().map(|m| m.guid.clone()).collect();
    let mut att_map = queries::get_message_attachments(&conn, &message_ids).unwrap_or_default();
    let mut reaction_map =
        queries::get_reactions_for_messages(&conn, &message_guids).unwrap_or_default();

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
            let attachments: Vec<AttachmentView> = att_map
                .remove(&m.id)
                .unwrap_or_default()
                .into_iter()
                .map(|a| {
                    let mime_type = a.mime_type;
                    let is_image = mime_type
                        .as_deref()
                        .map(|mime| mime.starts_with("image/"))
                        .unwrap_or(false);

                    AttachmentView {
                        id: a.id,
                        mime_type,
                        transfer_name: a.transfer_name,
                        total_bytes: a.total_bytes,
                        is_sticker: a.is_sticker,
                        is_image,
                    }
                })
                .collect();
            let image_attachment_count = attachments
                .iter()
                .filter(|att| att.is_image && !att.is_sticker)
                .count();
            let is_sticker_only = m.body.as_deref().unwrap_or_default().trim().is_empty()
                && !attachments.is_empty()
                && attachments.iter().all(|att| att.is_sticker);
            let reactions = build_reactions(reaction_map.remove(&m.guid).unwrap_or_default());
            let sender_name = m.sender_name.map(|name| format_contact_value(&name));
            MessageView {
                id: m.id,
                body_html: m.body.as_deref().map(linkify_text),
                is_from_me: m.is_from_me,
                service: m.service,
                sender_initial: sender_name.as_deref().map(display_initial),
                sender_name,
                has_attachments: m.has_attachments,
                time_formatted,
                date_formatted,
                attachments,
                reactions,
                use_attachment_grid: image_attachment_count > 1,
                is_sticker_only,
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
                format_contact_label(r.sender_name.as_deref(), r.sender_handle.as_deref())
            };
            let conversation_label = r
                .conversation_name
                .map(|label| format_contact_value(&label))
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
    initial: String,
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
                let name = format_contact_label(c.display_name.as_deref(), c.handle.as_deref());
                UnifiedConversationHit {
                    id: c.id,
                    initial: display_initial(&name),
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
                format_contact_label(r.sender_name.as_deref(), r.sender_handle.as_deref())
            };
            let conversation_label = r
                .conversation_name
                .map(|label| format_contact_value(&label))
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
    let page = params.page.unwrap_or(0);
    let offset = (page * ATTACHMENTS_PER_PAGE) as i64;
    let limit = (ATTACHMENTS_PER_PAGE + 1) as i64;

    let conn = state.db.lock().unwrap();
    let conversation_id = canonical_conversation_id(&conn, params.conversation_id);
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
