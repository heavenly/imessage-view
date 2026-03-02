# iMessage `chat.db` Schema Reference

> Production-quality technical reference for Apple's iMessage SQLite database.
> All claims backed by source evidence from [`imessage-exporter`](https://github.com/ReagentX/imessage-exporter) at commit [`1f7075608`](https://github.com/ReagentX/imessage-exporter/tree/1f7075608b328a4445311d7869e2af7a64a36419).

---

## 1. Database Overview

| Property | Value |
|----------|-------|
| **Format** | SQLite 3 |
| **macOS path** | `~/Library/Messages/chat.db` |
| **iOS backup path** | `3d/3d0d7e5fb2ce288813306e4d4636395e047a3d28` |
| **Access** | Read-only (Full Disk Access required on macOS) |

**Source**: [`table.rs#L259-L261`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/table.rs#L259-L261)

### Core Tables

| Table | Purpose |
|-------|---------|
| `message` | Every sent/received message |
| `chat` | Conversations (1:1 and group) |
| `handle` | Contacts (phone numbers, emails) |
| `attachment` | File metadata for media |
| `chat_message_join` | Links messages → chats |
| `chat_handle_join` | Links handles → chats |
| `message_attachment_join` | Links messages → attachments |
| `chat_recoverable_message_join` | Recently deleted messages (Ventura+) |

**Source**: [`table.rs#L221-L235`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/table.rs#L221-L235)

---

## 2. Core Tables & Columns

### 2.1 `message`

The 26 columns explicitly selected in the Ventura+ query (iOS 16+):

**Source**: [`message.rs#L162`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/messages/message.rs#L162)

| # | Column | Type | Nullable | Description |
|---|--------|------|----------|-------------|
| 0 | `rowid` | `INTEGER` | NO | Primary key (auto-increment) |
| 1 | `guid` | `TEXT` | NO | Globally unique identifier (UUID format) |
| 2 | `text` | `TEXT` | YES | Message body plaintext. **Often NULL on Ventura+** — must read `attributedBody` BLOB |
| 3 | `service` | `TEXT` | YES | `"iMessage"`, `"SMS"`, `"RCS"`, `"rcs"`, `"iMessageLite"` (satellite) |
| 4 | `handle_id` | `INTEGER` | YES | FK → `handle.rowid`. `0` = self in group chats |
| 5 | `destination_caller_id` | `TEXT` | YES | Address the DB owner received at (phone/email) |
| 6 | `subject` | `TEXT` | YES | Subject field (rarely used) |
| 7 | `date` | `INTEGER` | NO | Timestamp (see §6 Date Format) |
| 8 | `date_read` | `INTEGER` | YES | When message was read (0 if unread) |
| 9 | `date_delivered` | `INTEGER` | YES | When message was delivered (0 if undelivered) |
| 10 | `is_from_me` | `INTEGER` | NO | `1` = sent by DB owner, `0` = received |
| 11 | `is_read` | `INTEGER` | YES | `1` = read by recipient |
| 12 | `item_type` | `INTEGER` | YES | Combined with `group_action_type` for group events (see §8) |
| 13 | `other_handle` | `INTEGER` | YES | Target handle for group actions |
| 14 | `share_status` | `INTEGER` | YES | Whether shared data (e.g. location) is active |
| 15 | `share_direction` | `INTEGER` | YES | `0` = outgoing, `1` = incoming |
| 16 | `group_title` | `TEXT` | YES | New group name when chat is renamed |
| 17 | `group_action_type` | `INTEGER` | YES | Combined with `item_type` for group events (see §8) |
| 18 | `associated_message_guid` | `TEXT` | YES | Target GUID for tapbacks/replies (prefixed, see §9) |
| 19 | `associated_message_type` | `INTEGER` | YES | Type code: 0/2/3=normal, 1000-4000=tapback/vote (see §9) |
| 20 | `balloon_bundle_id` | `TEXT` | YES | App bundle ID for rich messages (see §12) |
| 21 | `expressive_send_style_id` | `TEXT` | YES | Bubble/screen effect ID (see §13) |
| 22 | `thread_originator_guid` | `TEXT` | YES | GUID of thread root message (replies) |
| 23 | `thread_originator_part` | `TEXT` | YES | Part index of replied-to content |
| 24 | `date_edited` | `INTEGER` | YES | Last edit timestamp (Ventura+, see §10) |
| 25 | `associated_message_emoji` | `TEXT` | YES | Custom emoji for tapback type 2006 |

**Source**: [`message.rs#L169-L234`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/messages/message.rs#L169-L234) (struct fields), [`message.rs#L415-L449`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/messages/message.rs#L415-L449) (index-based row mapping)

#### Additional BLOB Columns (not in COLS, read separately)

| Column | Type | Description |
|--------|------|-------------|
| `attributedBody` | `BLOB` | typedstream-encoded `NSAttributedString` (see §5) |
| `message_summary_info` | `BLOB` | plist with edit history (see §10) |
| `payload_data` | `BLOB` | plist with app/URL balloon data |

**Source**: [`table.rs#L239-L243`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/table.rs#L239-L243)

### 2.2 `chat`

| Column | Type | Nullable | Description |
|--------|------|----------|-------------|
| `rowid` | `INTEGER` | NO | Primary key |
| `chat_identifier` | `TEXT` | NO | Phone number, email, or group ID |
| `service_name` | `TEXT` | YES | `"iMessage"`, `"SMS"`, etc. |
| `display_name` | `TEXT` | YES | Custom group chat name (empty string = no name) |
| `properties` | `BLOB` | YES | plist with chat metadata (see §5.3) |

**Source**: [`chat.rs#L59-L68`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/chat.rs#L59-L68)

#### `properties` BLOB Structure (plist)

| Key | Type | Description |
|-----|------|-------------|
| `EnableReadReceiptForChat` | `Bool` | Read receipts enabled |
| `lastSeenMessageGuid` | `String` | GUID of most recent message |
| `shouldForceToSMS` | `Bool` | Force SMS/RCS instead of iMessage |
| `groupPhotoGuid` | `String` | GUID in attachment table for group photo |
| `backgroundProperties.trabar` | `String` | Chat background image ID |

**Source**: [`chat.rs#L26-L53`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/chat.rs#L26-L53)

### 2.3 `handle`

| Column | Type | Nullable | Description |
|--------|------|----------|-------------|
| `rowid` | `INTEGER` | NO | Primary key |
| `id` | `TEXT` | NO | Phone number or email address |
| `person_centric_id` | `TEXT` | YES | Groups handles that belong to the same person across services |

**Source**: [`handle.rs#L17-L24`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/handle.rs#L17-L24)

`handle_id = 0` in the message table means "self" (the database owner) in group chat contexts.

**Source**: [`handle.rs#L69-L70`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/handle.rs#L69-L70)

### 2.4 `attachment`

| Column | Type | Nullable | Description |
|--------|------|----------|-------------|
| `rowid` | `INTEGER` | NO | Primary key |
| `filename` | `TEXT` | YES | Full path on disk (macOS: `~/Library/Messages/Attachments/...`) |
| `uti` | `TEXT` | YES | Apple [Uniform Type Identifier](https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/understanding_utis/understand_utis_intro/understand_utis_intro.html) |
| `mime_type` | `TEXT` | YES | MIME type string (e.g. `"image/jpeg"`) |
| `transfer_name` | `TEXT` | YES | Original filename at send/receive time |
| `total_bytes` | `INTEGER` | NO | Bytes transferred over network |
| `is_sticker` | `INTEGER` | YES | `1` if attachment is a sticker |
| `hide_attachment` | `INTEGER` | YES | `1` to hide in UI |
| `emoji_image_short_description` | `TEXT` | YES | Genmoji prompt text |
| `sticker_user_info` | `BLOB` | YES | plist with sticker metadata |
| `attribution_info` | `BLOB` | YES | plist with sticker attribution |

**Source**: [`attachment.rs#L41`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/attachment.rs#L41) (COLS), [`attachment.rs#L91-L112`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/attachment.rs#L91-L112) (struct)

---

## 3. Join Tables & Relationships

### 3.1 `chat_message_join`

| Column | Type | Description |
|--------|------|-------------|
| `chat_id` | `INTEGER` | FK → `chat.rowid` |
| `message_id` | `INTEGER` | FK → `message.rowid` |

Most messages belong to exactly one chat. Some edge cases: messages in 0 chats ("orphaned") or >1 chats.

### 3.2 `chat_handle_join`

| Column | Type | Description |
|--------|------|-------------|
| `chat_id` | `INTEGER` | FK → `chat.rowid` |
| `handle_id` | `INTEGER` | FK → `handle.rowid` |

**Source**: [`chat_handle.rs#L16-L19`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/chat_handle.rs#L16-L19)

### 3.3 `message_attachment_join`

| Column | Type | Description |
|--------|------|-------------|
| `message_id` | `INTEGER` | FK → `message.rowid` |
| `attachment_id` | `INTEGER` | FK → `attachment.rowid` |

### 3.4 `chat_recoverable_message_join` (Ventura+ / iOS 16+)

| Column | Type | Description |
|--------|------|-------------|
| `chat_id` | `INTEGER` | FK → `chat.rowid` the message was deleted from |
| `message_id` | `INTEGER` | FK → `message.rowid` of the deleted message |

Messages appear here when "recently deleted" (30-day recovery window). The `chat_id` column tells you which chat it was deleted from.

**Source**: [`table.rs#L235`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/table.rs#L235), [`query_parts.rs#L28`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/messages/query_parts.rs#L28)

### 3.5 `chat_lookup` (Sequoia+)

Used to merge chat IDs that are split across services (e.g., same conversation on iMessage and RCS). Contains `chat` and `identifier` columns. A recursive CTE query finds the canonical chat ID by computing the transitive closure of shared identifiers.

**Source**: [`chat_handle.rs#L176-L200`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/chat_handle.rs#L176-L200)

---

## 4. Entity-Relationship Diagram

```
┌─────────┐     ┌───────────────────┐     ┌──────────┐
│  handle  │     │ chat_handle_join  │     │   chat   │
│──────────│     │───────────────────│     │──────────│
│ rowid PK │◄────│ handle_id  FK     │     │ rowid PK │
│ id       │     │ chat_id    FK     │────►│ chat_id  │
│ person_  │     └───────────────────┘     │ service  │
│ centric_id│                              │ display  │
└─────────┘                                │ properties│
     ▲                                     └──────────┘
     │                                          ▲
     │ handle_id FK                             │ chat_id FK
     │                                          │
┌─────────────────┐   ┌───────────────────┐     │
│     message      │   │chat_message_join  │     │
│─────────────────│   │───────────────────│     │
│ rowid        PK │   │ message_id  FK    │─────┘
│ guid            │◄──│ chat_id     FK    │
│ text            │   └───────────────────┘
│ handle_id    FK │
│ date            │   ┌───────────────────────────────┐
│ attributedBody  │   │chat_recoverable_message_join  │
│ message_summary │   │───────────────────────────────│
│ payload_data    │   │ message_id  FK                │
│ ...             │   │ chat_id     FK                │
└─────────────────┘   └───────────────────────────────┘
     │
     │ message_id FK
     ▼
┌───────────────────────┐     ┌────────────┐
│message_attachment_join│     │ attachment │
│───────────────────────│     │────────────│
│ message_id     FK     │     │ rowid   PK │
│ attachment_id  FK     │────►│ filename   │
└───────────────────────┘     │ mime_type  │
                              │ uti        │
                              │ transfer_  │
                              │   name     │
                              │ total_bytes│
                              └────────────┘
```

---

## 5. BLOB Columns & Binary Formats

### 5.1 `attributedBody` — typedstream Format

The `attributedBody` column stores an `NSAttributedString` encoded in Apple's proprietary **typedstream** binary format. On Ventura+, the `text` column is frequently NULL, making this the primary source of message body text.

**Header** (always 16 bytes):
```
04 0b  73 74 72 65 61 6d 74 79 70 65 64  81 e8 03
│  │   └─────── "streamtyped" ──────────┘  │  └─ version
│  └─ length of "streamtyped" (11)         └─ version indicator
└─ magic byte
```

**Key concepts**:
- **Indicator bytes** (`0x81`–`0x86`): Mark different data types
- **Type tag cache**: Starts at index `0x92`, caches Objective-C `@encode` type strings
- **Archivable object cache**: Also starts at `0x92`, caches class name hierarchies
- **String types**: Use `@encode` notation — `+` = UTF-8, `*` = ASCII/C-string

**Important typedstream keys** (from `NSAttributedString` attributes):

| Key | Value Type | Description |
|-----|-----------|-------------|
| `__kIMFileTransferGUIDAttributeName` | `NSString` | Attachment GUID |
| `__kIMFilenameAttributeName` | `NSString` | Attachment filename |
| `__kIMInlineMediaHeightAttributeName` | `float/double` | Attachment height (points) |
| `__kIMInlineMediaWidthAttributeName` | `float/double` | Attachment width (points) |
| `IMAudioTranscription` | `NSString` | Audio message transcription |
| `__kIMTextEffectAttributeName` | `i64` | Text animation ID (see §14) |
| `__kIMMentionConfirmedMention` | `NSString` | Mentioned contact's handle |

**Source**: [`models.rs#L165-L176`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/messages/models.rs#L165-L176) (key constants)

**External reference**: [Chris Sardegna's typedstream reverse engineering](https://chrissardegna.com/blog/reverse-engineering-apples-typedstream-format/), [`crabstep` crate](https://github.com/ReagentX/crabstep)

### 5.2 `message_summary_info` — Edited Message Plist

Binary plist containing edit/unsend history. Structure:

```
{
  "otr": {                           // Message body parts
    "0": { ... },                    // Part index → metadata
    "1": { ... }
  },
  "ec": {                            // Edit event history
    "0": [                           // Part index → array of events
      {
        "d": 740000000,              // Timestamp (seconds since 2001, multiply by TIMESTAMP_FACTOR)
        "t": <binary typedstream>,   // attributedBody of this edit revision
        "bcg": "GUID-string"         // Optional: balloon GUID reference
      }
    ]
  },
  "rp": [0, 1]                      // Indexes of unsent parts
}
```

- `otr` keys = message body part indexes; their count determines the number of parts
- `ec` = edit events; each entry's `d` timestamp must be multiplied by `1_000_000_000` (TIMESTAMP_FACTOR)
- `rp` = retracted parts; integer array of part indexes that were unsent
- Up to **5 edits** within **15 minutes** of sending; unsend within **2 minutes**

**Source**: [`edited.rs#L80-L108`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/message_types/edited.rs#L80-L108) (documentation), [`edited.rs#L116-L188`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/message_types/edited.rs#L116-L188) (parser)

### 5.3 `payload_data` — App Message Plist

NSKeyedArchiver-encoded plist for rich message balloons (URLs, Apple Pay, third-party apps, polls). Contents vary by `balloon_bundle_id`.

### 5.4 `properties` — Chat Metadata Plist

See §2.2 `chat` table above.

---

## 6. Date Format & Timestamps

All timestamp columns use the **Apple Core Data epoch**: seconds since `2001-01-01 00:00:00 UTC`.

### Detection: Nanoseconds vs Seconds

```
if value >= 1_000_000_000_000:
    seconds_since_2001 = value / 1_000_000_000   # Nanosecond precision (modern)
else:
    seconds_since_2001 = value                     # Second precision (legacy)
```

### Conversion Formula

```
unix_timestamp = seconds_since_2001 + 978307200
```

Where `978307200` = seconds between Unix epoch (`1970-01-01`) and Apple epoch (`2001-01-01`).

**Source**: [`dates.rs#L18`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/util/dates.rs#L18) (`TIMESTAMP_FACTOR = 1_000_000_000`), [`dates.rs#L33-L37`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/util/dates.rs#L33-L37) (`get_offset()`), [`dates.rs#L52-L65`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/util/dates.rs#L52-L65) (`get_local_time()`)

### SQL Example

```sql
-- Convert chat.db timestamp to Unix timestamp (seconds)
SELECT
    datetime(
        CASE
            WHEN date >= 1000000000000 THEN date / 1000000000
            ELSE date
        END + 978307200,
        'unixepoch',
        'localtime'
    ) AS readable_date
FROM message;
```

---

## 7. Attachments

### macOS Path Resolution

Attachment `filename` values use `~` prefix:
```
~/Library/Messages/Attachments/ab/12/GUID/filename.jpg
```

On macOS, expand `~` to the user's home directory.

**Source**: [`attachment.rs#L36-L38`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/attachment.rs#L36-L38) (path constants)

### iOS Backup Path Resolution

iOS backups use a **SHA-1 hash** scheme:
1. Take `"MediaDomain-" + relative_path` (where `relative_path` has `~` stripped)
2. Compute SHA-1 hex digest
3. First 2 hex characters = subdirectory name
4. File is at `<backup_root>/<first_2_chars>/<full_sha1_hash>`

**Source**: [`attachment.rs#L340-L343`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/attachment.rs#L340-L343)

### MIME Type Fallback

1. Check `mime_type` column → split on `/` → categorize (`image`, `video`, `audio`, `text`, `application`)
2. If `mime_type` is NULL, check `uti`:
   - `"com.apple.coreaudio-format"` → `"audio/x-caf; codecs=opus"` (voice messages)
3. If both NULL → `Unknown`

**Source**: [`attachment.rs#L183-L214`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/attachment.rs#L183-L214)

---

## 8. Group Chats vs 1:1

### Distinguishing Groups from 1:1

- **1:1 chat**: Exactly 1 entry in `chat_handle_join` for that `chat_id`
- **Group chat**: 2+ entries in `chat_handle_join` for that `chat_id`

```sql
SELECT c.rowid, c.chat_identifier, c.display_name,
       COUNT(chj.handle_id) AS participant_count
FROM chat c
JOIN chat_handle_join chj ON c.rowid = chj.chat_id
GROUP BY c.rowid
HAVING participant_count > 1;  -- Groups only
```

### Group Action Codes

Group actions are encoded as `(item_type, group_action_type)` tuples on the `message` table, with `other_handle` identifying the target participant:

| `item_type` | `group_action_type` | Action | `other_handle` |
|-------------|---------------------|--------|----------------|
| 1 | 0 | Participant added | Handle ID of added person |
| 1 | 0 | Phone number changed | Handle ID (when `handle_id == other_handle`) |
| 1 | 1 | Participant removed | Handle ID of removed person |
| 2 | * | Group name changed | — (`group_title` has new name) |
| 3 | 0 | Participant left | — |
| 3 | 1 | Group icon changed | — |
| 3 | 2 | Group icon removed | — |
| 3 | 4 | Chat background changed | — |
| 3 | 6 | Chat background removed | — |

**Special case**: When `item_type=1, group_action_type=0` and `handle_id == other_handle`, the sender changed their own phone number (not a participant add).

**Source**: [`models.rs#L203-L227`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/messages/models.rs#L203-L227)

---

## 9. Reactions & Tapbacks

### How Tapbacks Work

Tapbacks look like normal messages in the database. **Only the latest state is stored.** When a tapback is removed, the "add" row is **deleted** (not updated), causing non-sequential ROWIDs. History of tapback state changes is not preserved.

**Source**: [`variants.rs#L20-L29`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/message_types/variants.rs#L20-L29)

### `associated_message_type` Codes

| Code | Action | Tapback |
|------|--------|---------|
| 1000 | Added | Sticker (legacy) |
| 2000 | Added | ❤️ Loved |
| 2001 | Added | 👍 Liked |
| 2002 | Added | 👎 Disliked |
| 2003 | Added | 😂 Laughed |
| 2004 | Added | ‼️ Emphasized |
| 2005 | Added | ❓ Questioned |
| 2006 | Added | Custom Emoji (value in `associated_message_emoji`) |
| 2007 | Added | Sticker tapback |
| 3000 | Removed | ❤️ Loved |
| 3001 | Removed | 👍 Liked |
| 3002 | Removed | 👎 Disliked |
| 3003 | Removed | 😂 Laughed |
| 3004 | Removed | ‼️ Emphasized |
| 3005 | Removed | ❓ Questioned |
| 3006 | Removed | Custom Emoji |
| 3007 | Removed | Sticker tapback |
| 4000 | — | Poll vote (not a tapback) |

**Source**: [`message.rs#L1200-L1227`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/messages/message.rs#L1200-L1227)

### `associated_message_type` for Normal Messages

| Code | Meaning |
|------|---------|
| 0 | Standard text or app payload |
| 2 | App message variant |
| 3 | App message variant |

**Source**: [`message.rs#L1149-L1150`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/messages/message.rs#L1149-L1150)

### `associated_message_guid` Prefix Format

| Prefix | Target Type | Example |
|--------|-------------|---------|
| `p:<idx>/` | Normal message body part | `p:0/GUID` = first text/attachment, `p:2/GUID` = third body part |
| `bp:` | Bubble message (URL preview, app) | `bp:GUID` |

The index maps to `BubbleComponent` order: if a message has 3 attachments then text, index 0-2 are the attachments, index 3 is the text.

**Source**: [`variants.rs#L36-L45`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/message_types/variants.rs#L36-L45)

---

## 10. Edited & Unsent Messages

### Detection

A message has been edited when `date_edited != 0`. The edit data is stored in the `message_summary_info` BLOB (see §5.2).

### Edit Status Types

| Status | Description |
|--------|-------------|
| `Edited` | Content was modified (history in `ec` array) |
| `Unsent` | Content was retracted (part index in `rp` array) |
| `Original` | Content was not changed |

**Source**: [`edited.rs#L23-L31`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/message_types/edited.rs#L23-L31)

### Limits

- Max **5 edits** within **15 minutes** of sending
- Unsend within **2 minutes** of sending
- When ALL parts are unsent, the message becomes an `Announcement::FullyUnsent`

**Source**: [`edited.rs#L84-L86`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/message_types/edited.rs#L84-L86)

---

## 11. Deleted Messages & Recovery

### `chat_recoverable_message_join` Table

Present on **macOS Ventura+ / iOS 16+**. Messages moved to "Recently Deleted" remain recoverable for ~30 days.

- The `message_id` FK points to the message row (which still exists in the `message` table)
- The `chat_id` FK tells you which chat it was deleted from
- On the `Message` struct, this surfaces as `deleted_from`

### Query for Deleted Messages

```sql
SELECT m.*, d.chat_id AS deleted_from_chat
FROM message m
JOIN chat_recoverable_message_join d ON m.rowid = d.message_id
ORDER BY m.date;
```

**Source**: [`query_parts.rs#L17-L30`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/messages/query_parts.rs#L17-L30)

---

## 12. Message Variants & Special Types

### `balloon_bundle_id` Values

| Bundle ID | Type |
|-----------|------|
| `com.apple.messages.URLBalloonProvider` | URL preview |
| `com.apple.Handwriting.HandwritingProvider` | Handwritten message |
| `com.apple.DigitalTouchBalloonProvider` | Digital Touch |
| `com.apple.PassbookUIService.PeerPaymentMessagesExtension` | Apple Pay |
| `com.apple.ActivityMessagesApp.MessagesExtension` | Fitness.app |
| `com.apple.mobileslideshow.PhotosMessagesApp` | Photos slideshow |
| `com.apple.SafetyMonitorApp.SafetyMonitorMessages` | Check In |
| `com.apple.findmy.FindMyMessagesApp` | Find My |
| `com.apple.messages.Polls` | Poll (iOS 26+) |
| *(anything else)* | Third-party app |

**Source**: [`message.rs#L1174-L1196`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/messages/message.rs#L1174-L1196)

**URL overrides**: The `URLBalloonProvider` bundle ID is overloaded for Apple Music, App Store, Collaboration, and SharedPlacemark messages. Distinguish by examining `payload_data` contents.

**Source**: [`variants.rs#L110-L122`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/message_types/variants.rs#L110-L122)

### Service Values

| `service` column | Meaning |
|-----------------|---------|
| `"iMessage"` | Apple iMessage |
| `"SMS"` | Traditional SMS |
| `"RCS"` / `"rcs"` | RCS messaging |
| `"iMessageLite"` | Satellite messaging |
| *(other)* | Legacy (Jabber, IRC, etc.) |
| `NULL` | Unknown |

**Source**: [`models.rs#L52-L67`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/messages/models.rs#L52-L67)

### Polls (iOS 26+)

Poll messages use `balloon_bundle_id = "com.apple.messages.Polls"`. The poll itself is the message where `associated_message_guid == guid` (self-referential). Updates have a different GUID. Votes are separate messages with `associated_message_type = 4000`.

The poll payload in `payload_data` is an NSKeyedArchiver plist containing a `URL` key with a base64-encoded JSON payload describing poll options, votes, and creators.

**Source**: [`polls.rs#L80-L100`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/message_types/polls.rs#L80-L100)

---

## 13. Expressive Effects

### Bubble Effects (`expressive_send_style_id`)

| ID | Effect |
|----|--------|
| `com.apple.MobileSMS.expressivesend.impact` | Slam |
| `com.apple.MobileSMS.expressivesend.loud` | Loud |
| `com.apple.MobileSMS.expressivesend.gentle` | Gentle |
| `com.apple.MobileSMS.expressivesend.invisibleink` | Invisible Ink |

### Screen Effects (`expressive_send_style_id`)

| ID | Effect |
|----|--------|
| `com.apple.messages.effect.CKConfettiEffect` | Confetti |
| `com.apple.messages.effect.CKEchoEffect` | Echo |
| `com.apple.messages.effect.CKFireworksEffect` | Fireworks |
| `com.apple.messages.effect.CKHappyBirthdayEffect` | Balloons (birthday) |
| `com.apple.messages.effect.CKHeartEffect` | Heart |
| `com.apple.messages.effect.CKLasersEffect` | Lasers |
| `com.apple.messages.effect.CKShootingStarEffect` | Shooting Star |
| `com.apple.messages.effect.CKSparklesEffect` | Sparkles |
| `com.apple.messages.effect.CKSpotlightEffect` | Spotlight |

**Source**: [`expressives.rs#L48-L64`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/message_types/expressives.rs#L48-L64)

---

## 14. Text Effects & Formatting (iOS 18+)

Text effects are stored in the `attributedBody` typedstream under specific attribute keys.

### Animation IDs (`__kIMTextEffectAttributeName`)

| ID | Animation |
|----|-----------|
| 4 | Ripple |
| 5 | Big |
| 6 | Bloom |
| 8 | Nod |
| 9 | Shake |
| 10 | Jitter |
| 11 | Small |
| 12 | Explode |

**Source**: [`text_effects.rs#L110-L123`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/message_types/text_effects.rs#L110-L123)

### Text Styles

| Style | Description |
|-------|-------------|
| Bold | `**text**` |
| Italic | `*text*` |
| Strikethrough | `~~text~~` |
| Underline | underlined text |

### Other Text Effects

| Effect | Key/Mechanism | Data |
|--------|--------------|------|
| Mention | `__kIMMentionConfirmedMention` | Contact handle string |
| Link | URL detection | URL string |
| OTP | One-time code detection | — |
| Unit Conversion | Inline conversion | Currency, Distance, Temperature, Timezone, Volume, Weight |

**Source**: [`text_effects.rs#L11-L97`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/message_types/text_effects.rs#L11-L97)

---

## 15. Schema Version Compatibility Matrix

Three distinct schema tiers, detected by table/column presence:

| Feature | iOS 13 / Catalina | iOS 14-15 / Big Sur-Monterey | iOS 16+ / Ventura+ |
|---------|-------------------|------------------------------|---------------------|
| **Query style** | `SELECT *` | `SELECT *` | Explicit 26 columns |
| `thread_originator_guid` | ❌ | ✅ | ✅ |
| `chat_recoverable_message_join` | ❌ | ❌ | ✅ |
| `date_edited` | ❌ | ❌ | ✅ |
| `associated_message_emoji` | ❌ | ❌ | ✅ |
| `chat_lookup` | ❌ | ❌ | ✅ (Sequoia+) |
| Reply threading | ❌ | ✅ | ✅ |
| Deleted message recovery | ❌ | ❌ | ✅ |
| Edit/unsend support | ❌ | ❌ | ✅ |
| RCS support | ❌ | ❌ | ✅ (Sequoia+) |
| Satellite (`iMessageLite`) | ❌ | ❌ | ✅ (Sequoia+) |
| Text effects (animations/styles) | ❌ | ❌ | ✅ (iOS 18+) |
| Polls | ❌ | ❌ | ✅ (iOS 26+) |
| Genmoji (`emoji_image_short_description`) | ❌ | ❌ | ✅ (iOS 18+) |

**Schema detection strategy** (from imessage-exporter): Try queries in order from newest → oldest. First query that succeeds determines the schema tier.

**Source**: [`query_parts.rs#L1-L96`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/messages/query_parts.rs#L1-L96), [`message.rs#L244-L249`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/messages/message.rs#L244-L249)

---

## 16. Tricky & Undocumented Columns

### `is_from_me`

Always `0` or `1`. For tapbacks, `is_from_me = 1` means the DB owner applied the tapback, not that the original message was from the DB owner.

### `handle_id = 0`

Means "self" (the database owner) in group chat contexts. The handle cache pre-populates ID 0 → "Me".

**Source**: [`handle.rs#L69-L70`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/handle.rs#L69-L70)

### `cache_has_attachments`

Not used by imessage-exporter. Instead, attachment count is computed dynamically:
```sql
(SELECT COUNT(*) FROM message_attachment_join a WHERE m.ROWID = a.message_id) as num_attachments
```

### `person_centric_id`

Used to merge multiple handles (phone + email) belonging to the same contact. May not be present in all databases. When present, handles sharing a `person_centric_id` should be treated as the same person.

**Source**: [`handle.rs#L146-L156`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/handle.rs#L146-L156)

### `text` Column Being NULL

On Ventura+, the `text` column is frequently NULL even for normal text messages. You **must** read `attributedBody` and parse the typedstream to get the message body. The legacy `streamtyped` parser exists as a fallback for older format data.

**Source**: [`message.rs#L490-L557`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/messages/message.rs#L490-L557)

### Orphaned Messages

Messages can exist without a corresponding chat (no entry in `chat_message_join`). These are called "orphaned" messages. The diagnostic tools count them separately.

**Source**: [`message.rs#L276-L293`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/messages/message.rs#L276-L293), [`table.rs#L263`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/table.rs#L263)

### Chat Deduplication

A single conversation can have multiple `chat` rows (one per service, e.g., iMessage + SMS + RCS for the same contact). The `chat_lookup` table (Sequoia+) and `person_centric_id` on handles are used to merge these.

**Source**: [`chat_handle.rs#L159-L200`](https://github.com/ReagentX/imessage-exporter/blob/1f7075608b328a4445311d7869e2af7a64a36419/imessage-database/src/tables/chat_handle.rs#L159-L200)

---

## 17. Common Queries

### All messages in a conversation with sender info

```sql
SELECT
    m.rowid,
    m.guid,
    m.text,
    m.is_from_me,
    h.id AS sender,
    datetime(
        CASE WHEN m.date >= 1000000000000 THEN m.date / 1000000000 ELSE m.date END + 978307200,
        'unixepoch', 'localtime'
    ) AS sent_at
FROM message m
JOIN chat_message_join cmj ON m.rowid = cmj.message_id
LEFT JOIN handle h ON m.handle_id = h.rowid
WHERE cmj.chat_id = ?
ORDER BY m.date;
```

### All attachments for a message

```sql
SELECT a.rowid, a.filename, a.mime_type, a.transfer_name, a.total_bytes
FROM message_attachment_join maj
JOIN attachment a ON maj.attachment_id = a.rowid
WHERE maj.message_id = ?;
```

### All tapbacks on a message

```sql
SELECT
    m.associated_message_type,
    m.associated_message_emoji,
    m.is_from_me,
    h.id AS reactor
FROM message m
LEFT JOIN handle h ON m.handle_id = h.rowid
WHERE m.associated_message_guid LIKE '%' || ? || '%'
  AND m.associated_message_type BETWEEN 1000 AND 3999;
```

### Group chat participants

```sql
SELECT c.rowid, c.display_name, h.id AS participant
FROM chat c
JOIN chat_handle_join chj ON c.rowid = chj.chat_id
JOIN handle h ON chj.handle_id = h.rowid
WHERE c.rowid = ?;
```

### Recently deleted messages

```sql
SELECT m.*, d.chat_id AS deleted_from_chat
FROM message m
JOIN chat_recoverable_message_join d ON m.rowid = d.message_id
ORDER BY m.date DESC;
```

### Messages with edits

```sql
SELECT m.rowid, m.guid, m.text, m.date_edited
FROM message m
WHERE m.date_edited > 0
ORDER BY m.date_edited DESC;
```

---

## References

- **Primary source**: [ReagentX/imessage-exporter](https://github.com/ReagentX/imessage-exporter) (Rust, MIT license)
- **typedstream format**: [Chris Sardegna's reverse engineering blog post](https://chrissardegna.com/blog/reverse-engineering-apples-typedstream-format/)
- **typedstream parser**: [ReagentX/crabstep](https://github.com/ReagentX/crabstep)
- **Apple type encodings**: [Objective-C Runtime Guide](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/ObjCRuntimeGuide/Articles/ocrtTypeEncodings.html)
- **iOS backup file hashing**: [The Apple Wiki](https://theapplewiki.com/index.php?title=ITunes_Backup)
