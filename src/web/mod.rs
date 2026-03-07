pub mod attachments;
pub mod pages;
pub mod partials;
pub mod recovery;

use axum::routing::get;
use axum::Router;
use tower_http::services::ServeDir;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(pages::index))
        .route("/conversations/{id}", get(pages::conversation))
        .route("/conversations/{id}/photo", get(pages::conversation_photo))
        .route("/search", get(pages::search))
        .route("/attachments", get(pages::attachments_page))
        .route("/attachments/download/{id}", get(attachments::download))
        .route("/attachments/thumbnail/{id}", get(attachments::thumbnail))
        .route("/analytics", get(pages::analytics))
        .route("/recovery", get(recovery::recovery_page))
        .route("/partials/messages", get(partials::messages_partial))
        .route("/partials/conversations", get(partials::conversations_partial))
        .route("/partials/search-results", get(partials::search_results_partial))
        .route("/partials/conversation-attachments", get(partials::conversation_attachments_partial))
        .route("/partials/conversation-panel", get(partials::conversation_panel_partial))
        .nest_service("/static", ServeDir::new("static"))
}
