use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use std::path::Path as StdPath;
use tokio::fs::File;
use tokio::process::Command;
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

    let file_path = attachment.existing_path().ok_or(StatusCode::NOT_FOUND)?;

    let path = StdPath::new(file_path);

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

pub async fn preview(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, StatusCode> {
    let attachment = {
        let conn = state.db.lock().unwrap();
        queries::get_attachment(&conn, id).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };

    let attachment = attachment.ok_or(StatusCode::NOT_FOUND)?;
    let file_path = attachment.existing_path().ok_or(StatusCode::NOT_FOUND)?;
    let path = StdPath::new(file_path);
    let content_type = attachment
        .mime_type
        .as_deref()
        .unwrap_or("application/octet-stream");

    if content_type.starts_with("image/") {
        if should_transcode_image_preview(&attachment) {
            let data = generate_image_preview(path, 1800)
                .await
                .ok_or(StatusCode::NOT_FOUND)?;
            return Ok((
                [
                    (header::CONTENT_TYPE, "image/jpeg".to_string()),
                    (header::CONTENT_DISPOSITION, "inline".to_string()),
                    (header::CACHE_CONTROL, "public, max-age=3600".to_string()),
                ],
                Body::from(data),
            ));
        }
    }

    let file = File::open(path).await.map_err(|_| StatusCode::NOT_FOUND)?;
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    Ok((
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (header::CONTENT_DISPOSITION, "inline".to_string()),
            (header::CACHE_CONTROL, "public, max-age=3600".to_string()),
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

    let file_path = attachment.existing_path().ok_or(StatusCode::NOT_FOUND)?;

    let path = StdPath::new(file_path);

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
    generate_image_preview(path, 300).await
}

async fn generate_image_preview(path: &StdPath, max_dimension: u32) -> Option<Vec<u8>> {
    let decoded = tokio::task::spawn_blocking({
        let path = path.to_path_buf();
        move || image::ImageReader::open(&path).ok()?.decode().ok()
    })
    .await
    .ok()
    .flatten();

    let Some(img) = decoded else {
        return generate_image_preview_with_sips(path, max_dimension).await;
    };

    let thumbnail = img.thumbnail(max_dimension, max_dimension);

    // Convert to JPEG
    let mut buffer = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buffer);
    thumbnail
        .write_to(&mut cursor, image::ImageFormat::Jpeg)
        .ok()?;

    Some(buffer)
}

async fn generate_image_preview_with_sips(path: &StdPath, max_dimension: u32) -> Option<Vec<u8>> {
    let temp_file = tempfile::NamedTempFile::with_suffix(".jpg").ok()?;
    let temp_path = temp_file.path().to_path_buf();
    let max_dimension = max_dimension.to_string();

    let output = Command::new("sips")
        .args([
            "-s",
            "format",
            "jpeg",
            "-Z",
            max_dimension.as_str(),
            path.to_str()?,
            "--out",
            temp_path.to_str()?,
        ])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    tokio::fs::read(&temp_path).await.ok()
}

fn should_transcode_image_preview(attachment: &queries::AttachmentRow) -> bool {
    attachment
        .mime_type
        .as_deref()
        .map(|mime| {
            mime.eq_ignore_ascii_case("image/heic") || mime.eq_ignore_ascii_case("image/heif")
        })
        .unwrap_or(false)
        || attachment
            .display_name()
            .rsplit('.')
            .next()
            .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "heic" | "heif"))
            .unwrap_or(false)
}

async fn generate_video_thumbnail(path: &StdPath) -> Option<Vec<u8>> {
    // Try to extract first frame using ffmpeg
    // First check if ffmpeg is available
    let ffmpeg_check = Command::new("ffmpeg").arg("-version").output().await;

    if ffmpeg_check.is_err() {
        return None;
    }

    // Create a temporary file for the thumbnail
    let temp_file = tempfile::NamedTempFile::with_suffix(".jpg").ok()?;
    let temp_path = temp_file.path().to_path_buf();

    // Extract frame at 00:00:00.500 (500ms in)
    let result = Command::new("ffmpeg")
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
