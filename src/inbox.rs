/// Global Inbox — the app-level SQLite store for notes that haven't been
/// placed into a `.water` workspace yet.
///
/// Path: `~/.local/share/blot/inbox.db`
/// This is NOT a `.water` file. It is private to Blot and invisible to all
/// other Watercolor apps until the user places a note into a workspace.
use crate::note_version::NoteVersion;
use rusqlite::{params, Connection, Result};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Current schema version. Increment when making incompatible changes.
const SCHEMA_VERSION: i64 = 6;

// ── Pin and Recent entries ────────────────────────────────────────────────────

/// A globally-pinned note (inbox or workspace).
#[derive(Debug, Clone)]
pub struct PinEntry {
    pub id: String,
    /// `"inbox_note"` or `"workspace_note"`.
    pub target_kind: String,
    /// Note ID in the respective store.
    pub target_id: String,
    /// Empty string for inbox notes; absolute path to `.water` file otherwise.
    pub workspace_path: String,
    /// Cached note title (at pin time; may drift if note is later renamed).
    pub note_title: String,
    /// Cached first-line snippet.
    pub note_snippet: String,
    pub created_at: String,
    pub sort_order: i64,
}

/// A recently-accessed note, cached for display without querying workspace files.
#[derive(Debug, Clone)]
pub struct RecentEntry {
    pub id: String,
    pub target_kind: String,
    pub target_id: String,
    pub workspace_path: String,
    pub workspace_name: String,
    pub note_title: String,
    pub note_snippet: String,
    pub accessed_at: String,
}

// ── Absorbed file provenance ────────────────────────────────────────────────

/// Provenance record for an external `.txt` / `.md` file that was absorbed into
/// Blot. Stored in the Inbox DB regardless of destination so duplicate
/// detection works across the Inbox and all workspaces.
#[derive(Debug, Clone)]
pub struct AbsorbedFile {
    pub id: String,
    /// `"inbox_note"` or `"workspace_note"`.
    pub target_kind: String,
    /// Note ID in the respective store.
    pub target_id: String,
    /// Empty string for inbox notes; absolute `.water` path otherwise.
    pub workspace_path: String,
    /// Absolute path of the original source file at absorb time.
    pub source_file_path: String,
    /// Original file name (with extension).
    pub source_file_original_name: String,
    /// Original file's last-modified time (ISO-8601), if known.
    pub source_file_original_modified_at: Option<String>,
    /// When the absorb happened (ISO-8601).
    pub source_file_absorbed_at: String,
    /// `"left"` (file left in place) or `"trashed"` (moved to Trash).
    pub original_action: String,
}

// ── Inbox note ────────────────────────────────────────────────────────────────

/// A note stored in the Global Inbox.
#[derive(Debug, Clone)]
pub struct InboxNote {
    pub id: String,
    /// Display title — auto-derived or user-set.
    pub title: String,
    /// Full note body as plain/Markdown-ish text. Kept for search and display.
    pub body: String,
    /// Serialised `NoteDocument` JSON. The authoritative structured content.
    /// `None` for notes saved before Prompt 3 (they are re-parsed on next save).
    pub document_json: Option<String>,
    /// True when the title was auto-derived and may still be overwritten.
    pub auto_titled: bool,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
    /// ISO 8601 timestamp of last save.
    pub updated_at: String,
    pub word_count: i64,
    pub is_pinned: bool,
    pub is_archived: bool,
    // Placement tracking (set when moved to a workspace via Place Note).
    pub placed_at: Option<String>,
    pub placed_workspace_path: Option<String>,
    pub placed_workspace_note_id: Option<String>,
    pub placed_destination_label: Option<String>,
}

// ── Editor session state ──────────────────────────────────────────────────────

/// In-memory state for the note currently open in the editor.
/// Lives next to the EditorWidgets in main_window, not in the DB.
#[derive(Debug, Clone)]
pub struct NoteSession {
    /// DB id of the open note, or None if the note hasn't been saved yet.
    pub note_id: Option<String>,
    /// True while the title is auto-derived and can still be overwritten.
    pub auto_titled: bool,
    /// True if there are changes not yet persisted to the DB.
    pub dirty: bool,
    /// True if the current note is an Inbox note (not a workspace note).
    /// Used to decide whether Place Note UI elements should be active.
    pub is_inbox_note: bool,
}

impl Default for NoteSession {
    fn default() -> Self {
        NoteSession {
            note_id: None,
            auto_titled: true,
            dirty: false,
            is_inbox_note: true,
        }
    }
}

impl NoteSession {
    pub fn reset(&mut self) {
        *self = NoteSession::default();
    }
}

// ── Inbox database ────────────────────────────────────────────────────────────

pub struct InboxDb {
    conn: Connection,
}

impl InboxDb {
    /// Open (or create) the Inbox database at `path`.
    /// Creates parent directories and runs schema migrations automatically.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        // WAL mode: better concurrent reads and crash safety.
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = InboxDb { conn };
        db.migrate()?;
        Ok(db)
    }

    // ── Schema migrations ─────────────────────────────────────────────────

    fn migrate(&self) -> Result<()> {
        // Ensure the version tracking table exists before we read from it.
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS inbox_schema_version (version INTEGER NOT NULL);",
        )?;

        let version: i64 = self
            .conn
            .query_row(
                "SELECT version FROM inbox_schema_version LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        // V1 → create base tables (includes document_json from the start).
        if version < 1 {
            self.conn.execute_batch(SCHEMA_V1)?;
        }

        // V2 → add document_json to databases that were created at V1 without it.
        if version == 1 {
            // Ignore the error if the column already exists (e.g. re-migration).
            let _ = self
                .conn
                .execute_batch("ALTER TABLE inbox_notes ADD COLUMN document_json TEXT;");
        }

        // V3 → add global pins and recent-notes tables.
        if version < 3 {
            self.conn.execute_batch(SCHEMA_V3)?;
        }

        // V4 → add placement tracking columns to inbox_notes.
        if version < 4 {
            // ALTER TABLE is additive-only; ignore errors if columns already exist.
            let _ = self.conn.execute_batch(
                "ALTER TABLE inbox_notes ADD COLUMN placed_at TEXT;
                 ALTER TABLE inbox_notes ADD COLUMN placed_workspace_path TEXT;
                 ALTER TABLE inbox_notes ADD COLUMN placed_workspace_note_id TEXT;
                 ALTER TABLE inbox_notes ADD COLUMN placed_destination_label TEXT;",
            );
        }

        // V5 → add version snapshots table + merge-tracking columns.
        if version < 5 {
            self.conn.execute_batch(SCHEMA_V5)?;
            let _ = self.conn.execute_batch(
                "ALTER TABLE inbox_notes ADD COLUMN merged_into_kind TEXT;
                 ALTER TABLE inbox_notes ADD COLUMN merged_into_id TEXT;
                 ALTER TABLE inbox_notes ADD COLUMN merged_into_workspace_path TEXT;
                 ALTER TABLE inbox_notes ADD COLUMN merged_at TEXT;",
            );
        }

        // V6 → add the absorbed-files provenance table (tracks external .txt/.md
        // files absorbed into Blot, regardless of whether the destination was the
        // Inbox or a workspace).
        if version < 6 {
            self.conn.execute_batch(SCHEMA_V6)?;
        }

        if version < SCHEMA_VERSION {
            self.conn.execute(
                "INSERT INTO inbox_schema_version(version) VALUES (?1)
                 ON CONFLICT DO UPDATE SET version = excluded.version",
                params![SCHEMA_VERSION],
            )?;
        }

        Ok(())
    }

    // ── CRUD ──────────────────────────────────────────────────────────────

    /// Insert or update a note. Uses SQLite UPSERT: `created_at` is preserved
    /// on update, all other fields are overwritten.
    pub fn upsert_note(&self, note: &InboxNote) -> Result<()> {
        self.conn.execute(
            "INSERT INTO inbox_notes
               (id, title, body, document_json, auto_titled, created_at,
                updated_at, word_count, is_pinned, is_archived,
                placed_at, placed_workspace_path, placed_workspace_note_id,
                placed_destination_label)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(id) DO UPDATE SET
               title                    = excluded.title,
               body                     = excluded.body,
               document_json            = excluded.document_json,
               auto_titled              = excluded.auto_titled,
               updated_at               = excluded.updated_at,
               word_count               = excluded.word_count,
               is_pinned                = excluded.is_pinned,
               is_archived              = excluded.is_archived,
               placed_at                = excluded.placed_at,
               placed_workspace_path    = excluded.placed_workspace_path,
               placed_workspace_note_id = excluded.placed_workspace_note_id,
               placed_destination_label = excluded.placed_destination_label",
            params![
                note.id,
                note.title,
                note.body,
                note.document_json,
                note.auto_titled as i64,
                note.created_at,
                note.updated_at,
                note.word_count,
                note.is_pinned as i64,
                note.is_archived as i64,
                note.placed_at,
                note.placed_workspace_path,
                note.placed_workspace_note_id,
                note.placed_destination_label,
            ],
        )?;
        Ok(())
    }

    /// Archive an Inbox note as placed into a workspace.
    /// Sets is_archived=1 and records the placement destination metadata.
    /// Called as the final step of the Place Note transaction.
    pub fn mark_as_placed(
        &self,
        note_id: &str,
        workspace_path: &str,
        workspace_note_id: &str,
        destination_label: &str,
    ) -> Result<()> {
        let now = now_iso8601();
        let rows = self.conn.execute(
            "UPDATE inbox_notes
             SET is_archived              = 1,
                 placed_at               = ?1,
                 placed_workspace_path   = ?2,
                 placed_workspace_note_id = ?3,
                 placed_destination_label = ?4,
                 updated_at              = ?1
             WHERE id = ?5 AND is_archived = 0",
            params![
                now,
                workspace_path,
                workspace_note_id,
                destination_label,
                note_id
            ],
        )?;
        if rows == 0 {
            // Either the note doesn't exist or was already archived.
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        Ok(())
    }

    /// Return all non-archived notes, newest first.
    pub fn list_notes(&self) -> Result<Vec<InboxNote>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, body, document_json, auto_titled, created_at,
                    updated_at, word_count, is_pinned, is_archived,
                    placed_at, placed_workspace_path,
                    placed_workspace_note_id, placed_destination_label
             FROM inbox_notes
             WHERE is_archived = 0
             ORDER BY updated_at DESC",
        )?;
        let notes = stmt
            .query_map([], |row| {
                Ok(InboxNote {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    body: row.get(2)?,
                    document_json: row.get(3)?,
                    auto_titled: row.get::<_, i64>(4)? != 0,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                    word_count: row.get(7)?,
                    is_pinned: row.get::<_, i64>(8)? != 0,
                    is_archived: row.get::<_, i64>(9)? != 0,
                    placed_at: row.get(10)?,
                    placed_workspace_path: row.get(11)?,
                    placed_workspace_note_id: row.get(12)?,
                    placed_destination_label: row.get(13)?,
                })
            })?
            .collect::<Result<Vec<_>>>()?;
        Ok(notes)
    }

    /// Fetch a single note by ID. Returns `None` when not found.
    pub fn get_note(&self, id: &str) -> Result<Option<InboxNote>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, body, document_json, auto_titled, created_at,
                    updated_at, word_count, is_pinned, is_archived,
                    placed_at, placed_workspace_path,
                    placed_workspace_note_id, placed_destination_label
             FROM inbox_notes WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(InboxNote {
                id: row.get(0)?,
                title: row.get(1)?,
                body: row.get(2)?,
                document_json: row.get(3)?,
                auto_titled: row.get::<_, i64>(4)? != 0,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                word_count: row.get(7)?,
                is_pinned: row.get::<_, i64>(8)? != 0,
                is_archived: row.get::<_, i64>(9)? != 0,
                placed_at: row.get(10)?,
                placed_workspace_path: row.get(11)?,
                placed_workspace_note_id: row.get(12)?,
                placed_destination_label: row.get(13)?,
            })
        })?;
        match rows.next() {
            Some(Ok(note)) => Ok(Some(note)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    // ── Version snapshots ─────────────────────────────────────────────────────

    /// Snapshot the current state of `note` as a version record.
    pub fn create_version(
        &self,
        note: &InboxNote,
        reason: &str,
        is_bookmark: bool,
        bookmark_name: Option<&str>,
        bookmark_kind: Option<&str>,
        operation_id: Option<&str>,
    ) -> Result<NoteVersion> {
        let id = new_note_id();
        let now = now_iso8601();
        self.conn.execute(
            "INSERT INTO inbox_note_versions
               (id, note_id, title, body, document_json, created_at, reason,
                is_bookmark, bookmark_name, bookmark_kind, operation_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id,
                note.id,
                note.title,
                note.body,
                note.document_json,
                now,
                reason,
                is_bookmark as i64,
                bookmark_name,
                bookmark_kind,
                operation_id,
            ],
        )?;
        Ok(NoteVersion {
            id,
            note_id: note.id.clone(),
            title: note.title.clone(),
            body: note.body.clone(),
            document_json: note.document_json.clone(),
            created_at: now,
            reason: reason.to_string(),
            is_bookmark,
            bookmark_name: bookmark_name.map(str::to_string),
            bookmark_kind: bookmark_kind.map(str::to_string),
            operation_id: operation_id.map(str::to_string),
        })
    }

    /// Return all version records for a note, newest first.
    pub fn list_versions(&self, note_id: &str) -> Result<Vec<NoteVersion>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, note_id, title, body, document_json, created_at, reason,
                    is_bookmark, bookmark_name, bookmark_kind, operation_id
             FROM inbox_note_versions
             WHERE note_id = ?1
             ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map(params![note_id], |row| {
                Ok(NoteVersion {
                    id: row.get(0)?,
                    note_id: row.get(1)?,
                    title: row.get(2)?,
                    body: row.get(3)?,
                    document_json: row.get(4)?,
                    created_at: row.get(5)?,
                    reason: row.get(6)?,
                    is_bookmark: row.get::<_, i64>(7)? != 0,
                    bookmark_name: row.get(8)?,
                    bookmark_kind: row.get(9)?,
                    operation_id: row.get(10)?,
                })
            })?
            .collect::<Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Fetch a single version by ID. Returns `None` when not found.
    pub fn get_version(&self, version_id: &str) -> Result<Option<NoteVersion>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, note_id, title, body, document_json, created_at, reason,
                    is_bookmark, bookmark_name, bookmark_kind, operation_id
             FROM inbox_note_versions WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![version_id], |row| {
            Ok(NoteVersion {
                id: row.get(0)?,
                note_id: row.get(1)?,
                title: row.get(2)?,
                body: row.get(3)?,
                document_json: row.get(4)?,
                created_at: row.get(5)?,
                reason: row.get(6)?,
                is_bookmark: row.get::<_, i64>(7)? != 0,
                bookmark_name: row.get(8)?,
                bookmark_kind: row.get(9)?,
                operation_id: row.get(10)?,
            })
        })?;
        match rows.next() {
            Some(Ok(v)) => Ok(Some(v)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    /// Archive a note as merged into another note.
    /// Sets `is_archived = 1` and records the merge destination.
    pub fn archive_as_merged(
        &self,
        note_id: &str,
        merged_into_kind: &str,
        merged_into_id: &str,
        merged_into_workspace_path: &str,
    ) -> Result<()> {
        let now = now_iso8601();
        let rows = self.conn.execute(
            "UPDATE inbox_notes
             SET is_archived                  = 1,
                 merged_into_kind             = ?1,
                 merged_into_id               = ?2,
                 merged_into_workspace_path   = ?3,
                 merged_at                    = ?4,
                 updated_at                   = ?4
             WHERE id = ?5 AND is_archived = 0",
            params![
                merged_into_kind,
                merged_into_id,
                merged_into_workspace_path,
                now,
                note_id
            ],
        )?;
        if rows == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        Ok(())
    }

    // ── Pins ──────────────────────────────────────────────────────────────────

    /// Pin a note globally. Stores title/snippet as cached display metadata.
    /// If the note is already pinned the title/snippet are refreshed.
    pub fn pin_note(
        &self,
        target_kind: &str,
        target_id: &str,
        workspace_path: &str,
        note_title: &str,
        note_snippet: &str,
    ) -> Result<()> {
        let id = new_note_id();
        let now = now_iso8601();
        self.conn.execute(
            "INSERT INTO blot_pins
               (id, target_kind, target_id, workspace_path, note_title, note_snippet, created_at, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)
             ON CONFLICT(target_kind, target_id, workspace_path) DO UPDATE SET
               note_title   = excluded.note_title,
               note_snippet = excluded.note_snippet",
            params![id, target_kind, target_id, workspace_path, note_title, note_snippet, now],
        )?;
        Ok(())
    }

    /// Remove a global pin. Silently succeeds if the note was not pinned.
    pub fn unpin_note(
        &self,
        target_kind: &str,
        target_id: &str,
        workspace_path: &str,
    ) -> Result<()> {
        self.conn.execute(
            "DELETE FROM blot_pins
             WHERE target_kind = ?1 AND target_id = ?2 AND workspace_path = ?3",
            params![target_kind, target_id, workspace_path],
        )?;
        Ok(())
    }

    /// Return `true` when the note has a global pin entry.
    pub fn is_note_pinned(&self, target_kind: &str, target_id: &str, workspace_path: &str) -> bool {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM blot_pins
                 WHERE target_kind = ?1 AND target_id = ?2 AND workspace_path = ?3",
                params![target_kind, target_id, workspace_path],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0
    }

    /// Return all global pins ordered by sort_order then created_at.
    pub fn list_pins(&self) -> Result<Vec<PinEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, target_kind, target_id, workspace_path,
                    note_title, note_snippet, created_at, sort_order
             FROM blot_pins
             ORDER BY sort_order ASC, created_at ASC",
        )?;
        let pins = stmt
            .query_map([], |row| {
                Ok(PinEntry {
                    id: row.get(0)?,
                    target_kind: row.get(1)?,
                    target_id: row.get(2)?,
                    workspace_path: row.get(3)?,
                    note_title: row.get(4)?,
                    note_snippet: row.get(5)?,
                    created_at: row.get(6)?,
                    sort_order: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>>>()?;
        Ok(pins)
    }

    // ── Recent notes ──────────────────────────────────────────────────────────

    /// Record (or refresh) a recent note access. Upserts on (kind, id, path).
    pub fn touch_recent(&self, entry: &RecentEntry) -> Result<()> {
        self.conn.execute(
            "INSERT INTO blot_recent
               (id, target_kind, target_id, workspace_path, workspace_name,
                note_title, note_snippet, accessed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(target_kind, target_id, workspace_path) DO UPDATE SET
               workspace_name = excluded.workspace_name,
               note_title     = excluded.note_title,
               note_snippet   = excluded.note_snippet,
               accessed_at    = excluded.accessed_at",
            params![
                entry.id,
                entry.target_kind,
                entry.target_id,
                entry.workspace_path,
                entry.workspace_name,
                entry.note_title,
                entry.note_snippet,
                entry.accessed_at,
            ],
        )?;
        Ok(())
    }

    /// Return up to `limit` recent entries, newest first.
    pub fn list_recent(&self, limit: usize) -> Result<Vec<RecentEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, target_kind, target_id, workspace_path, workspace_name,
                    note_title, note_snippet, accessed_at
             FROM blot_recent
             ORDER BY accessed_at DESC
             LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(RecentEntry {
                    id: row.get(0)?,
                    target_kind: row.get(1)?,
                    target_id: row.get(2)?,
                    workspace_path: row.get(3)?,
                    workspace_name: row.get(4)?,
                    note_title: row.get(5)?,
                    note_snippet: row.get(6)?,
                    accessed_at: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ── Absorbed-file provenance ───────────────────────────────────────────────

    /// Record that an external file was absorbed into a note.
    pub fn record_absorption(&self, rec: &AbsorbedFile) -> Result<()> {
        self.conn.execute(
            "INSERT INTO absorbed_files
               (id, target_kind, target_id, workspace_path, source_file_path,
                source_file_original_name, source_file_original_modified_at,
                source_file_absorbed_at, original_action)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                rec.id,
                rec.target_kind,
                rec.target_id,
                rec.workspace_path,
                rec.source_file_path,
                rec.source_file_original_name,
                rec.source_file_original_modified_at,
                rec.source_file_absorbed_at,
                rec.original_action,
            ],
        )?;
        Ok(())
    }

    /// Find prior absorptions of the given source file path. Used to warn about
    /// duplicate absorbs. Newest first.
    pub fn find_absorptions_by_source_path(&self, source_path: &str) -> Result<Vec<AbsorbedFile>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, target_kind, target_id, workspace_path, source_file_path,
                    source_file_original_name, source_file_original_modified_at,
                    source_file_absorbed_at, original_action
             FROM absorbed_files
             WHERE source_file_path = ?1
             ORDER BY source_file_absorbed_at DESC",
        )?;
        let rows = stmt
            .query_map(params![source_path], |row| {
                Ok(AbsorbedFile {
                    id: row.get(0)?,
                    target_kind: row.get(1)?,
                    target_id: row.get(2)?,
                    workspace_path: row.get(3)?,
                    source_file_path: row.get(4)?,
                    source_file_original_name: row.get(5)?,
                    source_file_original_modified_at: row.get(6)?,
                    source_file_absorbed_at: row.get(7)?,
                    original_action: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Fetch the provenance record for a given absorbed note, if any.
    pub fn get_absorption_for_note(
        &self,
        target_kind: &str,
        target_id: &str,
    ) -> Result<Option<AbsorbedFile>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, target_kind, target_id, workspace_path, source_file_path,
                    source_file_original_name, source_file_original_modified_at,
                    source_file_absorbed_at, original_action
             FROM absorbed_files
             WHERE target_kind = ?1 AND target_id = ?2
             ORDER BY source_file_absorbed_at DESC",
        )?;
        let mut rows = stmt.query_map(params![target_kind, target_id], |row| {
            Ok(AbsorbedFile {
                id: row.get(0)?,
                target_kind: row.get(1)?,
                target_id: row.get(2)?,
                workspace_path: row.get(3)?,
                source_file_path: row.get(4)?,
                source_file_original_name: row.get(5)?,
                source_file_original_modified_at: row.get(6)?,
                source_file_absorbed_at: row.get(7)?,
                original_action: row.get(8)?,
            })
        })?;
        match rows.next() {
            Some(Ok(r)) => Ok(Some(r)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }
}

// ── Schema SQL ────────────────────────────────────────────────────────────────

const SCHEMA_V1: &str = "
CREATE TABLE IF NOT EXISTS inbox_notes (
    id            TEXT    PRIMARY KEY NOT NULL,
    title         TEXT    NOT NULL DEFAULT '',
    body          TEXT    NOT NULL DEFAULT '',
    document_json TEXT,
    auto_titled   INTEGER NOT NULL DEFAULT 1,
    created_at    TEXT    NOT NULL,
    updated_at    TEXT    NOT NULL,
    word_count    INTEGER NOT NULL DEFAULT 0,
    is_pinned     INTEGER NOT NULL DEFAULT 0,
    is_archived   INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS inbox_note_revisions (
    id         TEXT NOT NULL PRIMARY KEY,
    note_id    TEXT NOT NULL,
    body       TEXT NOT NULL,
    title      TEXT NOT NULL,
    created_at TEXT NOT NULL,
    reason     TEXT NOT NULL DEFAULT '',
    FOREIGN KEY(note_id) REFERENCES inbox_notes(id)
);

CREATE INDEX IF NOT EXISTS idx_inbox_notes_updated ON inbox_notes(updated_at DESC);
";

/// V3 migration: global pins and recent-notes cache tables.
const SCHEMA_V3: &str = "
CREATE TABLE IF NOT EXISTS blot_pins (
    id             TEXT NOT NULL PRIMARY KEY,
    target_kind    TEXT NOT NULL,
    target_id      TEXT NOT NULL,
    workspace_path TEXT NOT NULL DEFAULT '',
    note_title     TEXT NOT NULL DEFAULT '',
    note_snippet   TEXT NOT NULL DEFAULT '',
    created_at     TEXT NOT NULL,
    sort_order     INTEGER NOT NULL DEFAULT 0,
    UNIQUE(target_kind, target_id, workspace_path)
);

CREATE TABLE IF NOT EXISTS blot_recent (
    id             TEXT NOT NULL PRIMARY KEY,
    target_kind    TEXT NOT NULL,
    target_id      TEXT NOT NULL,
    workspace_path TEXT NOT NULL DEFAULT '',
    workspace_name TEXT NOT NULL DEFAULT '',
    note_title     TEXT NOT NULL DEFAULT '',
    note_snippet   TEXT NOT NULL DEFAULT '',
    accessed_at    TEXT NOT NULL,
    UNIQUE(target_kind, target_id, workspace_path)
);
";

/// V5 migration: version snapshot table for inbox notes.
const SCHEMA_V5: &str = "
CREATE TABLE IF NOT EXISTS inbox_note_versions (
    id            TEXT NOT NULL PRIMARY KEY,
    note_id       TEXT NOT NULL,
    title         TEXT NOT NULL DEFAULT '',
    body          TEXT NOT NULL DEFAULT '',
    document_json TEXT,
    created_at    TEXT NOT NULL,
    reason        TEXT NOT NULL DEFAULT '',
    is_bookmark   INTEGER NOT NULL DEFAULT 0,
    bookmark_name TEXT,
    bookmark_kind TEXT,
    operation_id  TEXT,
    FOREIGN KEY(note_id) REFERENCES inbox_notes(id)
);

CREATE INDEX IF NOT EXISTS idx_inbox_note_versions_note
    ON inbox_note_versions(note_id, created_at DESC);
";

/// V6 migration: provenance for external files absorbed into Blot.
const SCHEMA_V6: &str = "
CREATE TABLE IF NOT EXISTS absorbed_files (
    id                               TEXT NOT NULL PRIMARY KEY,
    target_kind                      TEXT NOT NULL,
    target_id                        TEXT NOT NULL,
    workspace_path                   TEXT NOT NULL DEFAULT '',
    source_file_path                 TEXT NOT NULL,
    source_file_original_name        TEXT NOT NULL DEFAULT '',
    source_file_original_modified_at TEXT,
    source_file_absorbed_at          TEXT NOT NULL,
    original_action                  TEXT NOT NULL DEFAULT 'left'
);

CREATE INDEX IF NOT EXISTS idx_absorbed_files_source
    ON absorbed_files(source_file_path);
";

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Generate a unique note ID without requiring an external crate.
/// Format: 16 hex timestamp nanos + 16 hex counter = 32-char string.
pub fn new_note_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    format!("{t:016x}{n:016x}")
}

/// Current time as an ISO 8601 string, using GLib's calendar.
pub fn now_iso8601() -> String {
    glib::DateTime::now_local()
        .and_then(|dt| dt.format_iso8601())
        .map(|s| s.to_string())
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

/// Shorten an ISO 8601 timestamp to a readable date: "2026-05-21".
pub fn format_date_short(iso: &str) -> &str {
    // ISO 8601 dates start with YYYY-MM-DD (10 chars).
    if iso.len() >= 10 {
        &iso[..10]
    } else {
        iso
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn open_temp_db() -> InboxDb {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_inbox.db");
        // tempdir is held alive long enough because InboxDb owns the Connection
        // which in turn holds the file open.
        let db = InboxDb::open(&path).expect("open temp db");
        // Leak the tempdir so the path survives. Acceptable for tests.
        std::mem::forget(dir);
        db
    }

    fn sample_note(id: &str) -> InboxNote {
        InboxNote {
            id: id.to_string(),
            title: "Test note".to_string(),
            body: "Hello world".to_string(),
            document_json: None,
            auto_titled: true,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            word_count: 2,
            is_pinned: false,
            is_archived: false,
            placed_at: None,
            placed_workspace_path: None,
            placed_workspace_note_id: None,
            placed_destination_label: None,
        }
    }

    #[test]
    fn db_opens_and_creates_schema() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("inbox.db");
        let _db = InboxDb::open(&path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn upsert_and_list() {
        let db = open_temp_db();
        let note = sample_note("note-1");
        db.upsert_note(&note).unwrap();

        let notes = db.list_notes().unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, "note-1");
        assert_eq!(notes[0].title, "Test note");
    }

    #[test]
    fn upsert_updates_existing_note() {
        let db = open_temp_db();
        db.upsert_note(&sample_note("n1")).unwrap();

        let mut updated = sample_note("n1");
        updated.title = "New title".to_string();
        updated.updated_at = "2026-06-01T00:00:00Z".to_string();
        db.upsert_note(&updated).unwrap();

        let notes = db.list_notes().unwrap();
        assert_eq!(notes.len(), 1, "should not duplicate");
        assert_eq!(notes[0].title, "New title");
    }

    #[test]
    fn created_at_preserved_on_upsert() {
        let db = open_temp_db();
        let mut note = sample_note("n1");
        note.created_at = "2026-01-01T00:00:00Z".to_string();
        db.upsert_note(&note).unwrap();

        // Upsert with a different created_at — it should NOT overwrite.
        let mut note2 = sample_note("n1");
        note2.created_at = "2099-01-01T00:00:00Z".to_string();
        note2.updated_at = "2026-06-01T00:00:00Z".to_string();
        db.upsert_note(&note2).unwrap();

        let fetched = db.get_note("n1").unwrap().unwrap();
        assert_eq!(fetched.created_at, "2026-01-01T00:00:00Z");
    }

    #[test]
    fn get_note_missing_returns_none() {
        let db = open_temp_db();
        assert!(db.get_note("nope").unwrap().is_none());
    }

    #[test]
    fn archived_notes_not_in_list() {
        let db = open_temp_db();
        let mut note = sample_note("n1");
        note.is_archived = true;
        db.upsert_note(&note).unwrap();
        assert!(db.list_notes().unwrap().is_empty());
    }

    #[test]
    fn list_sorted_newest_first() {
        let db = open_temp_db();
        let mut a = sample_note("a");
        a.updated_at = "2026-01-01T00:00:00Z".to_string();
        let mut b = sample_note("b");
        b.updated_at = "2026-06-01T00:00:00Z".to_string();
        db.upsert_note(&a).unwrap();
        db.upsert_note(&b).unwrap();

        let notes = db.list_notes().unwrap();
        assert_eq!(notes[0].id, "b");
        assert_eq!(notes[1].id, "a");
    }

    #[test]
    fn new_note_id_is_unique() {
        let a = new_note_id();
        let b = new_note_id();
        assert_ne!(a, b);
        assert_eq!(a.len(), 32);
    }

    // ── Pin tests ─────────────────────────────────────────────────────────────

    #[test]
    fn pin_inbox_note_and_list() {
        let db = open_temp_db();
        db.upsert_note(&sample_note("n1")).unwrap();

        db.pin_note("inbox_note", "n1", "", "Test note", "Hello world")
            .unwrap();

        let pins = db.list_pins().unwrap();
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].target_kind, "inbox_note");
        assert_eq!(pins[0].target_id, "n1");
        assert_eq!(pins[0].workspace_path, "");
        assert_eq!(pins[0].note_title, "Test note");
    }

    #[test]
    fn pin_workspace_note_and_list() {
        let db = open_temp_db();
        db.pin_note(
            "workspace_note",
            "wn1",
            "/my/ws.water",
            "WS Note",
            "snippet",
        )
        .unwrap();

        let pins = db.list_pins().unwrap();
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].target_kind, "workspace_note");
        assert_eq!(pins[0].workspace_path, "/my/ws.water");
    }

    #[test]
    fn unpin_note_removes_entry() {
        let db = open_temp_db();
        db.pin_note("inbox_note", "n1", "", "Title", "snippet")
            .unwrap();
        assert!(db.is_note_pinned("inbox_note", "n1", ""));

        db.unpin_note("inbox_note", "n1", "").unwrap();
        assert!(!db.is_note_pinned("inbox_note", "n1", ""));
        assert!(db.list_pins().unwrap().is_empty());
    }

    #[test]
    fn pin_same_note_twice_does_not_duplicate() {
        let db = open_temp_db();
        db.pin_note("inbox_note", "n1", "", "Title A", "snippet")
            .unwrap();
        db.pin_note("inbox_note", "n1", "", "Title B updated", "new snippet")
            .unwrap();

        let pins = db.list_pins().unwrap();
        assert_eq!(pins.len(), 1, "no duplicate");
        assert_eq!(pins[0].note_title, "Title B updated", "metadata refreshed");
    }

    #[test]
    fn unpin_nonexistent_is_ok() {
        let db = open_temp_db();
        // Should not error
        db.unpin_note("inbox_note", "ghost", "").unwrap();
    }

    // ── Recent tests ──────────────────────────────────────────────────────────

    fn sample_recent(kind: &str, id: &str, accessed_at: &str) -> RecentEntry {
        RecentEntry {
            id: new_note_id(),
            target_kind: kind.to_string(),
            target_id: id.to_string(),
            workspace_path: "".to_string(),
            workspace_name: "Inbox".to_string(),
            note_title: format!("Note {id}"),
            note_snippet: "snippet".to_string(),
            accessed_at: accessed_at.to_string(),
        }
    }

    #[test]
    fn recent_notes_ordered_newest_first() {
        let db = open_temp_db();
        db.touch_recent(&sample_recent("inbox_note", "a", "2026-05-01T00:00:00Z"))
            .unwrap();
        db.touch_recent(&sample_recent("inbox_note", "b", "2026-05-10T00:00:00Z"))
            .unwrap();
        db.touch_recent(&sample_recent("inbox_note", "c", "2026-05-05T00:00:00Z"))
            .unwrap();

        let recents = db.list_recent(10).unwrap();
        assert_eq!(recents.len(), 3);
        assert_eq!(recents[0].target_id, "b");
        assert_eq!(recents[1].target_id, "c");
        assert_eq!(recents[2].target_id, "a");
    }

    #[test]
    fn touch_recent_upserts_metadata() {
        let db = open_temp_db();
        let mut entry = sample_recent("inbox_note", "n1", "2026-05-01T00:00:00Z");
        db.touch_recent(&entry).unwrap();

        entry.note_title = "Updated Title".to_string();
        entry.accessed_at = "2026-05-20T00:00:00Z".to_string();
        db.touch_recent(&entry).unwrap();

        let recents = db.list_recent(10).unwrap();
        assert_eq!(recents.len(), 1, "no duplicate");
        assert_eq!(recents[0].note_title, "Updated Title");
        assert_eq!(recents[0].accessed_at, "2026-05-20T00:00:00Z");
    }

    #[test]
    fn list_recent_respects_limit() {
        let db = open_temp_db();
        for i in 0..20 {
            let mut e = sample_recent(
                "inbox_note",
                &i.to_string(),
                &format!("2026-05-{:02}T00:00:00Z", i + 1),
            );
            e.target_id = format!("note-{i}");
            db.touch_recent(&e).unwrap();
        }
        let recents = db.list_recent(5).unwrap();
        assert_eq!(recents.len(), 5);
    }

    #[test]
    fn note_session_resets() {
        let mut s = NoteSession {
            note_id: Some("x".into()),
            auto_titled: false,
            dirty: true,
            is_inbox_note: true,
        };
        s.reset();
        assert!(s.note_id.is_none());
        assert!(s.auto_titled);
        assert!(!s.dirty);
        assert!(s.is_inbox_note);
    }

    #[test]
    fn mark_as_placed_archives_note() {
        let db = open_temp_db();
        db.upsert_note(&sample_note("n1")).unwrap();

        db.mark_as_placed("n1", "/ws/test.water", "ws-note-id-123", "MyWS → Main Room")
            .unwrap();

        // Should no longer appear in active list.
        assert!(db.list_notes().unwrap().is_empty());

        // Fetching directly should show it archived.
        let note = db.get_note("n1").unwrap().unwrap();
        assert!(note.is_archived);
        assert_eq!(
            note.placed_workspace_path.as_deref(),
            Some("/ws/test.water")
        );
        assert_eq!(
            note.placed_workspace_note_id.as_deref(),
            Some("ws-note-id-123")
        );
        assert_eq!(
            note.placed_destination_label.as_deref(),
            Some("MyWS → Main Room")
        );
        assert!(note.placed_at.is_some());
    }

    #[test]
    fn mark_as_placed_on_nonexistent_note_errors() {
        let db = open_temp_db();
        let result = db.mark_as_placed("ghost", "/ws/test.water", "note-id", "Dest");
        assert!(result.is_err());
    }

    #[test]
    fn mark_as_placed_on_already_archived_note_errors() {
        let db = open_temp_db();
        let mut note = sample_note("n1");
        note.is_archived = true;
        db.upsert_note(&note).unwrap();

        let result = db.mark_as_placed("n1", "/ws/test.water", "note-id", "Dest");
        assert!(result.is_err(), "double-placing should fail");
    }

    // ── Version snapshot tests ─────────────────────────────────────────────────

    #[test]
    fn create_and_list_versions() {
        let db = open_temp_db();
        let note = sample_note("n1");
        db.upsert_note(&note).unwrap();

        db.create_version(&note, "before split", false, None, None, None)
            .unwrap();
        db.create_version(
            &note,
            "manual bookmark",
            true,
            Some("My Bookmark"),
            Some("manual"),
            None,
        )
        .unwrap();

        let versions = db.list_versions("n1").unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].bookmark_name.as_deref(), Some("My Bookmark"));
        assert_eq!(versions[1].reason, "before split");
    }

    #[test]
    fn get_version_by_id() {
        let db = open_temp_db();
        let note = sample_note("n1");
        db.upsert_note(&note).unwrap();
        let v = db
            .create_version(&note, "test", false, None, None, None)
            .unwrap();

        let fetched = db.get_version(&v.id).unwrap().unwrap();
        assert_eq!(fetched.id, v.id);
        assert_eq!(fetched.note_id, "n1");
    }

    #[test]
    fn get_version_missing_returns_none() {
        let db = open_temp_db();
        assert!(db.get_version("ghost").unwrap().is_none());
    }

    #[test]
    fn versions_for_wrong_note_not_returned() {
        let db = open_temp_db();
        let n1 = sample_note("n1");
        let n2 = sample_note("n2");
        db.upsert_note(&n1).unwrap();
        db.upsert_note(&n2).unwrap();
        db.create_version(&n1, "snap", false, None, None, None)
            .unwrap();

        assert_eq!(db.list_versions("n2").unwrap().len(), 0);
    }

    #[test]
    fn archive_as_merged_archives_note() {
        let db = open_temp_db();
        db.upsert_note(&sample_note("n1")).unwrap();
        db.archive_as_merged("n1", "inbox_note", "n2", "").unwrap();

        assert!(db.list_notes().unwrap().is_empty());
        let note = db.get_note("n1").unwrap().unwrap();
        assert!(note.is_archived);
    }

    #[test]
    fn archive_as_merged_nonexistent_errors() {
        let db = open_temp_db();
        assert!(db
            .archive_as_merged("ghost", "inbox_note", "n2", "")
            .is_err());
    }

    // ── Absorbed-file provenance tests ──────────────────────────────────────────

    fn sample_absorbed(source_path: &str, kind: &str, target: &str) -> AbsorbedFile {
        AbsorbedFile {
            id: new_note_id(),
            target_kind: kind.to_string(),
            target_id: target.to_string(),
            workspace_path: String::new(),
            source_file_path: source_path.to_string(),
            source_file_original_name: "note.txt".to_string(),
            source_file_original_modified_at: Some("2026-01-01T00:00:00Z".to_string()),
            source_file_absorbed_at: "2026-06-01T00:00:00Z".to_string(),
            original_action: "left".to_string(),
        }
    }

    #[test]
    fn record_and_find_absorption() {
        let db = open_temp_db();
        db.record_absorption(&sample_absorbed("/home/u/note.txt", "inbox_note", "n1"))
            .unwrap();

        let found = db
            .find_absorptions_by_source_path("/home/u/note.txt")
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].target_kind, "inbox_note");
        assert_eq!(found[0].target_id, "n1");
        assert_eq!(found[0].original_action, "left");
    }

    #[test]
    fn find_absorption_for_unknown_path_is_empty() {
        let db = open_temp_db();
        assert!(db
            .find_absorptions_by_source_path("/nope.txt")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn duplicate_absorption_detectable() {
        let db = open_temp_db();
        db.record_absorption(&sample_absorbed("/x/dup.md", "inbox_note", "n1"))
            .unwrap();
        db.record_absorption(&sample_absorbed("/x/dup.md", "workspace_note", "wn1"))
            .unwrap();
        let found = db.find_absorptions_by_source_path("/x/dup.md").unwrap();
        assert_eq!(
            found.len(),
            2,
            "both absorptions of the same file are tracked"
        );
    }

    #[test]
    fn get_absorption_for_note_returns_record() {
        let db = open_temp_db();
        db.record_absorption(&sample_absorbed("/x/a.txt", "inbox_note", "n1"))
            .unwrap();
        let rec = db.get_absorption_for_note("inbox_note", "n1").unwrap();
        assert!(rec.is_some());
        assert_eq!(rec.unwrap().source_file_path, "/x/a.txt");
        assert!(db
            .get_absorption_for_note("inbox_note", "ghost")
            .unwrap()
            .is_none());
    }
}
