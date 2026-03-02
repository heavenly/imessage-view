use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use tokio::fs::File;
use tokio_util::io::ReaderStream;

use crate::db::queries;
use crate::state::AppState;

pub async fn download(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, StatusCode> {
    let attachment = {
        let conn = state.db.lock().unwrap();
        queries::get_attachment(&conn, id).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };

    let attachment = attachment.ok_or(StatusCode::NOT_FOUND)?;

    if !attachment.file_exists {
        return Err(StatusCode::NOT_FOUND);
    }

    let resolved = attachment.resolved_path.as_deref().ok_or(StatusCode::NOT_FOUND)?;
    let path = std::path::Path::new(resolved);

    if !path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    let file = File::open(path).await.map_err(|_| StatusCode::NOT_FOUND)?;
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let content_type = attachment
        .mime_type
        .as_deref()
        .unwrap_or("application/octet-stream");

    let display = attachment.display_name().to_string();
    let disposition = format!("attachment; filename=\"{display}\"");

    Ok((
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        body,
    ))
}
