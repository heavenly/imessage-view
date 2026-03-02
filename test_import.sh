#!/bin/bash

# Test import with error logging - only process first 1000 messages

cd /Users/sanjayk/Desktop/programming/imessage_db_port

# Backup existing db
mv data/imessage.db data/imessage.db.backup 2>/dev/null || true

echo "Running import with error logging..."
echo "=================================="

# Run import and capture first 100 lines of stderr
cargo run -- import 2>&1 | head -200

echo ""
echo "=================================="
echo "Checking imported messages..."
sqlite3 data/imessage.db "SELECT COUNT(*) as total, COUNT(body) as with_body FROM messages;"

echo ""
echo "Sample of messages with NULL body:"
sqlite3 data/imessage.db "SELECT apple_message_id, is_from_me, body IS NULL as body_null FROM messages WHERE body IS NULL LIMIT 10;"

echo ""
echo "Sample of messages WITH body:"
sqlite3 data/imessage.db "SELECT apple_message_id, is_from_me, substr(body, 1, 50) as body FROM messages WHERE body IS NOT NULL LIMIT 10;"