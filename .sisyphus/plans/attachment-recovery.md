# Missing Attachment Recovery - Implementation Plan

## Problem Summary
- **83,807 total attachments** in database
- **36,365 (43%) exist locally** ✓
- **47,442 (57%) are missing** ✗
- Most missing: Images (JPEG: 15,188, HEIC: 14,029, PNG: 6,545) and videos

## Root Cause
**iCloud Photo Library with "Optimize Mac Storage"** - macOS automatically removed full-resolution files, keeping only thumbnails. Database still references original paths.

## Implementation Options

### Option 1: Missing Attachment Report Page ⭐ RECOMMENDED
**Effort**: Low | **Value**: High

Add a new page `/attachments/missing` that shows:
- List of all missing attachments with metadata
- Grouped by conversation
- Sortable by date, type, size
- Export to CSV for manual recovery
- Search/filter by conversation name

**Files to modify/create**:
- `src/db/queries.rs` - Add `get_missing_attachments()` query
- `src/web/pages.rs` - Add `missing_attachments_page()` handler
- `src/web/mod.rs` - Add route
- `templates/missing_attachments.html` - Report UI
- `templates/base.html` - Add nav link

**Pros**: Immediately useful, helps manual recovery, low complexity
**Cons**: Doesn't auto-recover files

---

### Option 2: iCloud Recovery Indicator
**Effort**: Low | **Value**: Medium

Add visual indicators in attachment views showing iCloud sync status:
- 🟢 Local file exists
- ☁️ Synced to iCloud (recoverable from iCloud Photos)
- ❌ Missing (likely deleted)

Query `ck_sync_state` from source database:
- 0 = Not synced
- 1 = Synced to iCloud
- 2 = Pending upload
- 4 = Error

**Files to modify**:
- Import: Store `ck_sync_state` during import
- `src/db/queries.rs` - Add sync state to attachment queries
- CSS: Add status icons
- Templates: Show status badges

**Pros**: Visual clarity on recoverability
**Cons**: Requires re-import with new column

---

### Option 3: Time Machine Recovery Scanner
**Effort**: Medium | **Value**: High

Scan Time Machine backups for missing attachments:
1. User points to Time Machine backup path
2. Tool searches backup for missing filenames
3. Shows which files are recoverable from backups
4. Optionally copy found files back

**Files to modify/create**:
- `src/recovery/mod.rs` - New module
- `src/recovery/time_machine.rs` - Scanner logic
- CLI command: `cargo run -- recover --source /Volumes/TimeMachine`
- Web UI integration

**Pros**: Can actually recover files automatically
**Cons**: Requires Time Machine access, slower scanning

---

### Option 4: iOS Backup Scanner
**Effort**: Medium | **Value**: High

Scan iPhone/iPad backups for missing attachments:
1. User provides iOS backup directory
2. Tool maps iOS backup SHA-1 paths to attachment filenames
3. Extracts found attachments

**Files to modify/create**:
- `src/recovery/ios_backup.rs` - iOS backup parser
- CLI: `cargo run -- recover --ios-backup /path/to/backup`

**Pros**: iOS devices often have full-resolution images
**Cons**: Requires unencrypted backup, path mapping complexity

---

### Option 5: Photo Library Integration ⭐ ADVANCED
**Effort**: High | **Value**: Very High

Access macOS Photos Library directly to retrieve missing images:
- Use Photos Library SQLite database (`Photos Library.photoslibrary`)
- Match by creation date, filename, or UUID
- Export matched photos back to Messages attachments folder

**Files to modify/create**:
- `src/recovery/photos_library.rs` - Photos Library reader
- Complex UUID/filename matching logic

**Pros**: Can recover from iCloud-downloaded photos in library
**Cons**: Complex, fragile to macOS updates

---

## Recommendation

**Start with Option 1 (Missing Attachment Report)** - immediately useful, low risk

**Then Option 3 (Time Machine Scanner)** - if user has Time Machine backups

**Option 5 (Photos Library)** - advanced recovery for power users

---

## Database Schema Addition (for Options 2+)

```sql
ALTER TABLE attachments ADD COLUMN ck_sync_state INTEGER DEFAULT 0;
ALTER TABLE attachments ADD COLUMN ck_record_id TEXT;
```

---

## Next Steps

1. **Choose which option(s) to implement**
2. **Re-import with sync state** (if Option 2+)
3. **Implement chosen feature**
4. **Test recovery process**

Which option would you like to pursue?