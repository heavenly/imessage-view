use imessage_database::{
    tables::{
        messages::Message,
        table::{get_connection, Table},
    },
    util::query_context::QueryContext,
};
use std::collections::HashMap;

fn main() {
    let home = std::env::var("HOME").unwrap();
    let source_path = std::path::PathBuf::from(home).join("Library/Messages/chat.db");

    println!("Opening source database: {:?}", source_path);
    let conn = get_connection(&source_path).expect("Failed to connect");
    let context = QueryContext::default();

    println!("\nTesting generate_text on first 10 non-reaction messages:\n");

    let mut statement = Message::stream_rows(&conn, &context).expect("Failed to stream");
    let rows = statement
        .query_map([], |row| Ok(Message::from_row(row)))
        .expect("Failed to query");

    let mut count = 0;
    for message_result in rows {
        if count >= 10 {
            break;
        }

        let mut message = Message::extract(message_result).expect("Failed to extract");

        // Skip reactions
        if let Some(assoc_type) = message.associated_message_type {
            if (1000..=4000).contains(&assoc_type) {
                continue;
            }
        }

        count += 1;

        println!("Message rowid={}", message.rowid);
        println!(
            "  text column: {:?}",
            message.text.as_deref().unwrap_or("NULL")
        );

        match message.generate_text(&conn) {
            Ok(text) => {
                println!("  generate_text SUCCESS: len={}", text.len());
                if text.len() < 100 {
                    println!("  text preview: {:?}", text);
                }
            }
            Err(e) => {
                println!("  generate_text FAILED: {:?}", e);
            }
        }
        println!();
    }
}
