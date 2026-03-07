# Implementation Plan: iCloud Recovery Indicator + iOS Backup Scanner

## Overview
Implement Option 2 (iCloud Recovery Indicators) and Option 4 (iOS Backup Scanner) for missing attachment recovery.

**Current State:**
- 83,807 total attachments
- 36,365 exist locally (43%)
- 47,442 missing (57%)
- 80,126 have ck_sync_state=1 (synced to iCloud - recoverable!)

---

## Wave 1: Foundation (Sequential)

### Task 1: Schema Migration + Dependencies
**Duration**: 5 min | **Category**: quick

**What to do:**
1. Add `sha1` crate to Cargo.toml
2. Update `src/db/schema.rs` CREATE_ATTACHMENTS with new columns:
   - ck_sync_state INTEGER DEFAULT 0
   - ck_record_id TEXT
   - is_sticker BOOLEAN DEFAULT FALSE
   - hide_attachment BOOLEAN DEFAULT FALSE
   - backup_source_path TEXT

**Files:**
- Cargo.toml - add sha1 = "0.10"
- src/db/schema.rs:51-64 - extend CREATE_ATTACHMENTS

**Acceptance:**
- cargo check passes
- New columns present in schema

---

## Wave 2: Core Implementation (Parallel - 3 tasks)

### Task 2: Update Import Pipeline
**Duration**: 10 min | **Category**: quick
**Depends on**: Task 1

**What to do:**
Update `src/import/attachments.rs` to:
1. SELECT ck_sync_state, ck_record_id, is_sticker, hide_attachment from source
2. Handle NULL ck_sync_state with COALESCE(ck_sync_state, 0)
3. Add fields to AttachmentRow struct
4. Update INSERT to include new columns

**Files:**
- src/import/attachments.rs:37-44 - extend SELECT
- src/import/attachments.rs:9-20 - extend AttachmentRow struct
- src/import/attachments.rs:145-153 - extend INSERT

**Acceptance:**
- cargo build passes
- Re-import includes iCloud data

---

### Task 3: Update Queries
**Duration**: 15 min | **Category**: quick
**Depends on**: Task 1

**What to do:**
Update `src/db/queries.rs` to:
1. Add ck_sync_state to AttachmentRow struct
2. Update all 6 attachment queries to SELECT ck_sync_state
3. Update row mapping to include ck_sync_state

**Queries to update:**
- list_attachments (line 451)
- get_attachment (line 487)
- conversation_attachments (line 552)
- count_conversation_attachments (line 587)

**Files:**
- src/db/queries.rs:370-382 - extend AttachmentRow
- src/db/queries.rs:453,475 - add ck_sync_state to SELECTs
- src/db/queries.rs:490,508,554,575 - add ck_sync_state to SELECTs

**Acceptance:**
- cargo build passes
- All queries compile

---

### Task 4: Create Recovery Module
**Duration**: 20 min | **Category**: deep
**Depends on**: Task 1

**What to do:**
Create iOS backup scanner module:
1. Create `src/recovery/mod.rs` - module exports
2. Create `src/recovery/ios_backup.rs`:
   - `resolve_ios_backup_path(backup_root: &Path, original_path: &str) -> Option<PathBuf>`
   - `scan_for_attachment(backup_root: &Path, original_path: &str) -> Option<PathBuf>`
   - `copy_from_backup(src: &Path, dst: &Path) -> Result<()>`
3. SHA-1 implementation: sha1("MediaDomain-" + relative_path), first 2 chars as subdir

**Files:**
- src/recovery/mod.rs (new)
- src/recovery/ios_backup.rs (new)
- src/lib.rs or src/main.rs - add mod recovery

**Acceptance:**
- Unit tests pass
- Can resolve iOS backup paths correctly

---

## Wave 3: Backend Integration (Parallel - 3 tasks)

### Task 5: Update Views
**Duration**: 10 min | **Category**: quick
**Depends on**: Task 3

**What to do:**
Update `src/web/pages.rs` to:
1. Add ck_sync_state to AttachmentView struct
2. Map ck_sync_state to sync_status string ("local", "icloud", "pending", "error")
3. Update attachments_page() to populate sync_status

**Files:**
- src/web/pages.rs:207-219 - extend AttachmentView
- src/web/pages.rs:247-258 - map ck_sync_state to sync_status

**Acceptance:**
- cargo build passes
- AttachmentView has sync_status field

---

### Task 6: Update Download Handler
**Duration**: 10 min | **Category**: quick
**Depends on**: Task 4

**What to do:**
Update `src/web/attachments.rs` to:
1. Check backup_source_path as fallback when file_exists=false
2. Try to copy from backup to local if found
3. Stream from backup location if local copy fails

**Files:**
- src/web/attachments.rs:22-35 - add backup fallback logic

**Acceptance:**
- cargo build passes
- Download handler compiles

---

### Task 7: CLI Recovery Command
**Duration**: 15 min | **Category**: quick
**Depends on**: Task 4

**What to do:**
Add to `src/main.rs`:
1. New CLI command: `scan-ios-backup --backup-path <path>`
2. Implementation:
   - Query all attachments with file_exists=false
   - For each, check if exists in iOS backup using recovery module
   - Update backup_source_path in DB if found
   - Optionally copy files with --copy flag
3. Progress bar for scanning

**Files:**
- src/main.rs - add Commands::ScanIosBackup variant
- src/main.rs - implement scan_ios_backup function

**Acceptance:**
- cargo build passes
- CLI help shows new command

---

## Wave 4: UI (Parallel - 2 tasks)

### Task 8: Update Attachments Page
**Duration**: 15 min | **Category**: visual-engineering
**Depends on**: Task 5

**What to do:**
Update `templates/attachments.html`:
1. Add filter tabs: "All / Local / iCloud / Missing"
2. Add status badges on attachment cards:
   - 🟢 Local file
   - ☁️ iCloud only (show when ck_sync_state=1 and file_exists=false)
   - ❌ Missing
3. Add "Recover from iCloud" hint for cloud-synced missing files
4. Add CSS for status badges

**Files:**
- templates/attachments.html - add filter tabs (after line 37)
- templates/attachments.html - add status badges (in attachment-card, after line 44)
- static/css/style.css - add .sync-badge classes

**Acceptance:**
- Template renders without errors
- UI shows sync status

---

### Task 9: Create Recovery UI Page
**Duration**: 20 min | **Category**: visual-engineering
**Depends on**: Task 5, Task 7

**What to do:**
1. Create `src/web/recovery.rs` with:
   - recovery_page() handler showing missing attachments recoverable from iCloud
   - recover_attachment() handler to trigger recovery
2. Add routes in `src/web/mod.rs`:
   - GET /recovery - recovery page
   - POST /recovery/{id} - recover specific attachment
3. Create `templates/recovery.html`:
   - List missing attachments with iCloud sync state
   - Show "Recover" buttons for cloud-synced files
   - Show backup source path if known
   - Progress indicator for recovery operations

**Files:**
- src/web/recovery.rs (new)
- src/web/mod.rs - add routes
- templates/recovery.html (new)
- templates/base.html - add Recovery link to nav

**Acceptance:**
- Page loads at /recovery
- Shows missing attachments
- Recover button triggers recovery

---

## Execution Order

```
Wave 1: Task 1
    ↓
Wave 2: Task 2 + Task 3 + Task 4 (parallel)
    ↓
Wave 3: Task 5 + Task 6 + Task 7 (parallel)
    ↓
Wave 4: Task 8 + Task 9 (parallel)
```

## Testing Checklist

- [ ] cargo build passes after each wave
- [ ] Re-import populates ck_sync_state correctly
- [ ] Attachments page shows iCloud badges
- [ ] Filter tabs work (All/Local/iCloud/Missing)
- [ ] Recovery page lists missing iCloud attachments
- [ ] CLI scan-ios-backup finds files in backup
- [ ] Download handler falls back to backup path

## Rollback Plan

If issues occur:
1. Schema change is backward compatible (new columns have defaults)
2. Re-import recreates tables with new schema
3. Old code ignores new columns (no breaking changes)
