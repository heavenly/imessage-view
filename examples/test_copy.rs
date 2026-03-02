use imessage_database::{
    tables::{
        messages::Message,
        table::{get_connection, Table},
    },
    util::query_context::QueryContext,
};
use std::path::PathBuf;

fn main() {
    let home = std::env::var("HOME").unwrap();

    // Test original
    let original_path = PathBuf::from(home.clone()).join("Library/Messages/chat.db");
    println!("Testing ORIGINAL database: {:?}", original_path);
    test_db(&original_path, "original");

    println!("\n{}", "=".repeat(60));

    // Test copy
    let copy_path = PathBuf::from("data/source_chat.db");
    println!("Testing COPIED database: {:?}", copy_path);
    test_db(&copy_path, "copy");
}

fn test_db(path: &PathBuf, label: &str) {
    match get_connection(path) {
        Ok(conn) => {
            let context = QueryContext::default();

            match Message::stream_rows(&conn, &context) {
                Ok(mut statement) => {
                    let rows = statement
                        .query_map([], |row| Ok(Message::from_row(row)))
                        .expect("Failed to query");

                    let mut success = 0;
                    let mut failed = 0;

                    for (i, message_result) in rows.enumerate() {
                        if i >= 100 {
                            break;
                        } // Test first 100

                        if let Ok(mut message) = Message::extract(message_result) {
                            // Skip reactions
                            if let Some(assoc_type) = message.associated_message_type {
                                if (1000..=4000).contains(&assoc_type) {
                                    continue;
                                }
                            }

                            match message.generate_text(&conn) {
                                Ok(_) => success += 1,
                                Err(_) => failed += 1,
                            }
                        }
                    }

                    println!("  {}: {} succeeded, {} failed", label, success, failed);
                }
                Err(e) => println!("  {}: Failed to stream rows: {:?}", label, e),
            }
        }
        Err(e) => println!("  {}: Failed to connect: {:?}", label, e),
    }
}
