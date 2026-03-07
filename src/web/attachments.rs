use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use std::path::Path as StdPath;
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

    let file_path = if attachment.file_exists {
        attachment.resolved_path.as_deref().ok_or(StatusCode::NOT_FOUND)?
    } else if let Some(backup_path) = &attachment.backup_source_path {
        if StdPath::new(backup_path).exists() {
            backup_path.as_str()
        } else {
            return Err(StatusCode::NOT_FOUND);
        }
    } else {
        return Err(StatusCode::NOT_FOUND);
    };

    let path = StdPath::new(file_path);

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

/// Generate a thumbnail for an attachment
/// For images: resize to max 300x300
/// For videos: try to extract first frame using ffmpeg, fallback to placeholder
/// Returns None if thumbnail cannot be generated
pub async fn thumbnail(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, StatusCode> {
    let attachment = {
        let conn = state.db.lock().unwrap();
        queries::get_attachment(&conn, id).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };

    let attachment = attachment.ok_or(StatusCode::NOT_FOUND)?;

    // Only generate thumbnails for images and videos
    let mime_category = attachment.mime_category();
    if !matches!(mime_category, "image" | "video") {
        return Err(StatusCode::NOT_FOUND);
    }

    let file_path = if attachment.file_exists {
        attachment.resolved_path.as_deref().ok_or(StatusCode::NOT_FOUND)?
    } else if let Some(backup_path) = &attachment.backup_source_path {
        if StdPath::new(backup_path).exists() {
            backup_path.as_str()
        } else {
            return Err(StatusCode::NOT_FOUND);
        }
    } else {
        return Err(StatusCode::NOT_FOUND);
    };

    let path = StdPath::new(file_path);

    if !path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    // Generate thumbnail based on type
    let thumbnail_data = if mime_category == "image" {
        generate_image_thumbnail(path).await
    } else {
        generate_video_thumbnail(path).await
    };

    match thumbnail_data {
        Some(data) => Ok((
            [
                (header::CONTENT_TYPE, "image/jpeg".to_string()),
                (header::CACHE_CONTROL, "public, max-age=86400".to_string()),
            ],
            data,
        )),
        None => Err(StatusCode::NOT_FOUND),
    }
}

async fn generate_image_thumbnail(path: &StdPath) -> Option<Vec<u8>> {
    // Try to open and resize the image
    let img = tokio::task::spawn_blocking({
        let path = path.to_path_buf();
        move || {
            image::ImageReader::open(&path)
                .ok()?
                .decode()
                .ok()
        }
    })
    .await
    .ok()
    .flatten()?;

    // Resize to max 300x300 maintaining aspect ratio
    let thumbnail = img.thumbnail(300, 300);
    
    // Convert to JPEG
    let mut buffer = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buffer);
    thumbnail.write_to(&mut cursor, image::ImageFormat::Jpeg).ok()?;
    
    Some(buffer)
}

async fn generate_video_thumbnail(path: &StdPath) -> Option<Vec<u8>> {
    // Try to extract first frame using ffmpeg
    // First check if ffmpeg is available
    let ffmpeg_check = tokio::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .await;
    
    if ffmpeg_check.is_err() {
        return None;
    }

    // Create a temporary file for the thumbnail
    let temp_file = tempfile::NamedTempFile::with_suffix(".jpg").ok()?;
    let temp_path = temp_file.path().to_path_buf();

    // Extract frame at 00:00:00.500 (500ms in)
    let result = tokio::process::Command::new("ffmpeg")
        .args(&[
            "-i",
            path.to_str()?,
            "-ss",
            "00:00:00.500",
            "-vframes",
            "1",
            "-vf",
            "scale=300:300:force_original_aspect_ratio=decrease",
            "-q:v",
            "2",
            temp_path.to_str()?,
        ])
        .output()
        .await;

    match result {
        Ok(output) if output.status.success() => {
            // Read the generated thumbnail
            tokio::fs::read(&temp_path).await.ok()
        }
        _ => None,
    }
}