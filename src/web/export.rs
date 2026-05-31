use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Local};
use serde::Serialize;

use crate::db::queries;
use crate::state::AppState;

fn canonical_conversation_id(conn: &rusqlite::Connection, conversation_id: i64) -> i64 {
    queries::resolve_canonical_conversation_id(conn, conversation_id)
        .ok()
        .flatten()
        .unwrap_or(conversation_id)
}

#[derive(Debug)]
enum ExportScope {
    Both,
    Me,
    Others,
    Selected,
}

#[derive(Debug)]
struct ExportParams {
    scope: ExportScope,
    include_me: bool,
    participant_ids: Vec<i64>,
}

#[derive(Debug, Serialize)]
struct ExportedMessage {
    text: String,
    datetime: String,
}

fn query_values<'a>(query: &'a str, key: &str) -> Vec<&'a str> {
    query
        .split('&')
        .filter_map(|pair| {
            let (raw_key, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
            (raw_key == key).then_some(raw_value)
        })
        .collect()
}

fn query_value<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query_values(query, key).into_iter().next()
}

fn parse_export_params(query: &str) -> ExportParams {
    let scope = match query_value(query, "scope").unwrap_or("both") {
        "me" => ExportScope::Me,
        "others" => ExportScope::Others,
        "selected" => ExportScope::Selected,
        _ => ExportScope::Both,
    };
    let include_me = query_value(query, "include_me").is_some_and(|value| value != "0");
    let participant_ids = query_values(query, "participant_id")
        .into_iter()
        .filter_map(|value| value.parse::<i64>().ok())
        .collect();

    ExportParams {
        scope,
        include_me,
        participant_ids,
    }
}

fn format_export_datetime(unix: i64) -> String {
    DateTime::from_timestamp(unix, 0)
        .map(|dt| {
            dt.with_timezone(&Local)
                .format("%Y-%m-%dT%H:%M:%S")
                .to_string()
        })
        .unwrap_or_default()
}

fn error_response(status: StatusCode, message: &str) -> Response {
    (status, message.to_string()).into_response()
}

pub async fn export_messages(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    uri: Uri,
) -> Response {
    let params = parse_export_params(uri.query().unwrap_or_default());

    let result = {
        let conn = state.db.lock().unwrap();
        let conversation_id = canonical_conversation_id(&conn, id);
        let filter = match params.scope {
            ExportScope::Both => queries::ExportMessageFilter::All,
            ExportScope::Me => queries::ExportMessageFilter::Mine,
            ExportScope::Others => queries::ExportMessageFilter::Others,
            ExportScope::Selected => queries::ExportMessageFilter::Selected {
                include_me: params.include_me,
                contact_ids: &params.participant_ids,
            },
        };

        queries::export_messages(&conn, conversation_id, filter).map(|rows| (conversation_id, rows))
    };

    let (conversation_id, rows) = match result {
        Ok(result) => result,
        Err(err) => {
            eprintln!("failed to export messages for conversation {id}: {err}");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to export messages",
            );
        }
    };

    let messages: Vec<ExportedMessage> = rows
        .into_iter()
        .map(|row| ExportedMessage {
            text: row.text,
            datetime: format_export_datetime(row.date_unix),
        })
        .collect();

    let body = match serde_json::to_string_pretty(&messages) {
        Ok(body) => body,
        Err(err) => {
            eprintln!(
                "failed to serialize message export for conversation {conversation_id}: {err}"
            );
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to serialize export",
            );
        }
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    if let Ok(value) = HeaderValue::from_str(&format!(
        "attachment; filename=\"conversation-{conversation_id}-messages.json\""
    )) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }

    (headers, body).into_response()
}
