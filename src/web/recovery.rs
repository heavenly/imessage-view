use askama::Template;
use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse};
use serde::Deserialize;

use crate::db::queries;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct RecoveryQuery {
    pub page: Option<u32>,
}

#[derive(Template)]
#[template(path = "recovery.html")]
struct RecoveryTemplate {
    title: String,
    attachments: Vec<RecoveryAttachmentView>,
    page: u32,
    total_pages: u32,
    has_prev: bool,
    has_next: bool,
    total_missing: i64,
    icloud_count: i64,
    backup_count: i64,
}

struct RecoveryAttachmentView {
    id: i64,
    display_name: String,
    conversation_name: Option<String>,
    conversation_id: Option<i64>,
    date: String,
    size: String,
    sync_status: String,
    has_backup: bool,
}

const PER_PAGE: i64 = 50;

pub async fn recovery_page(
    State(state): State<AppState>,
    Query(params): Query<RecoveryQuery>,
) -> impl IntoResponse {
    let page = params.page.unwrap_or(1).max(1) as i64;
    let offset = (page - 1) * PER_PAGE;
    
    let conn = state.db.lock().unwrap();
    
    // Get counts
    let total_missing = queries::count_missing_attachments(&conn).unwrap_or(0);
    let icloud_count = queries::count_missing_icloud_attachments(&conn).unwrap_or(0);
    let backup_count = queries::count_missing_with_backup(&conn).unwrap_or(0);
    
    // Get missing attachments
    let rows = queries::get_missing_attachments(&conn, offset, PER_PAGE).unwrap_or_default();
    let total_pages = ((total_missing + PER_PAGE - 1) / PER_PAGE).max(1);
    
    let attachments: Vec<RecoveryAttachmentView> = rows
        .into_iter()
        .map(|a| RecoveryAttachmentView {
            id: a.id,
            display_name: a.display_name().to_string(),
            conversation_name: a.conversation_name.clone(),
            conversation_id: a.conversation_id,
            date: a.date_formatted(),
            size: a.human_size(),
            sync_status: match a.ck_sync_state {
                0 => "local".to_string(),
                1 => "icloud".to_string(),
                2 => "pending".to_string(),
                _ => "error".to_string(),
            },
            has_backup: a.backup_source_path.is_some(),
        })
        .collect();
    
    let t = RecoveryTemplate {
        title: "Attachment Recovery".to_string(),
        attachments,
        page: page as u32,
        total_pages: total_pages as u32,
        has_prev: page > 1,
        has_next: page < total_pages,
        total_missing,
        icloud_count,
        backup_count,
    };
    
    Html(t.render().unwrap_or_default())
}
