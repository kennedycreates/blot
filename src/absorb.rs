//! Absorb an external `.txt` / `.md` file into Blot's structured note system.
//!
//! "Absorbing" takes an [`ExternalFile`](crate::external_file::ExternalFile)
//! loaded in the editor and creates a real Blot note from it — either in the
//! Global Inbox or in a `.water` workspace (Loose Notes, a Shelf, or a Pile).
//!
//! Every absorb:
//! - parses the file content into a structured `NoteDocument` (`document_json`)
//! - preserves `created_at` / `updated_at`
//! - records source-file provenance in the Inbox DB ([`AbsorbedFile`])
//! - creates an auto-bookmark version snapshot ("Absorbed from file")
//!
//! Safety contract:
//! - If creating the note fails, nothing is recorded and the original file is
//!   untouched.
//! - The original file is never trashed or deleted here — the caller performs
//!   the explicit Leave/Trash step after a successful absorb.
//! - The auto-bookmark is best-effort: if it fails the absorb still succeeds
//!   (the note already exists); the failure is logged.

use crate::document;
use crate::external_file::ExternalFile;
use crate::inbox::{new_note_id, now_iso8601, AbsorbedFile, InboxDb, InboxNote};
use crate::place_note::PlaceDestination;
use crate::workspace::{new_id, NotePlacement, WorkspaceDb, WorkspaceNote};
use std::fmt;
use std::path::Path;

// ── Result / error types ──────────────────────────────────────────────────────

/// What happened to the original file after a successful absorb.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginalAction {
    /// File left where it is (safe default).
    Leave,
    /// File moved to the system Trash.
    Trash,
}

impl OriginalAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Leave => "left",
            Self::Trash => "trashed",
        }
    }
}

/// Outcome of a successful absorb.
#[derive(Debug, Clone)]
pub struct AbsorbResult {
    /// `"inbox_note"` or `"workspace_note"`.
    pub target_kind: String,
    pub target_id: String,
    /// Empty for inbox notes; `.water` path otherwise. Retained for callers
    /// that need to re-open the destination workspace.
    #[allow(dead_code)]
    pub workspace_path: String,
    /// Final title of the absorbed note.
    #[allow(dead_code)]
    pub title: String,
    /// Human-readable destination, e.g. `Inbox` or `MyWS → Main Room → Shelf X`.
    pub destination_label: String,
}

#[derive(Debug)]
pub enum AbsorbError {
    Inbox(rusqlite::Error),
    Workspace(crate::workspace::WorkspaceError),
    DestinationNotFound(String),
}

impl fmt::Display for AbsorbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inbox(e) => write!(f, "Inbox DB error: {e}"),
            Self::Workspace(e) => write!(f, "Workspace error: {e}"),
            Self::DestinationNotFound(s) => write!(f, "Destination not found: {s}"),
        }
    }
}

impl From<rusqlite::Error> for AbsorbError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Inbox(e)
    }
}

impl From<crate::workspace::WorkspaceError> for AbsorbError {
    fn from(e: crate::workspace::WorkspaceError) -> Self {
        Self::Workspace(e)
    }
}

// ── Absorb into Inbox ─────────────────────────────────────────────────────────

/// Absorb an external file into the Global Inbox.
///
/// `title` is the user-confirmed title (typically from
/// [`crate::external_file::derive_file_title`]). `original_action` records what
/// the caller intends to do with the source file (it does **not** perform the
/// trash/leave itself — only records provenance).
pub fn absorb_into_inbox(
    inbox_db: &InboxDb,
    ef: &ExternalFile,
    title: &str,
    original_action: OriginalAction,
) -> Result<AbsorbResult, AbsorbError> {
    let now = now_iso8601();
    let doc = document::markdown::parse(&ef.content);
    let document_json = serde_json::to_string(&doc).ok();
    let word_count = ef.content.split_whitespace().count() as i64;

    let note_id = new_note_id();
    let note = InboxNote {
        id: note_id.clone(),
        title: title.to_string(),
        body: ef.content.clone(),
        document_json,
        // File-absorbed notes carry a deliberate, file-derived title that we do
        // not want silently overwritten by smart-title on the next edit.
        auto_titled: false,
        created_at: now.clone(),
        updated_at: now.clone(),
        word_count,
        is_pinned: false,
        is_archived: false,
        placed_at: None,
        placed_workspace_path: None,
        placed_workspace_note_id: None,
        placed_destination_label: None,
    };
    inbox_db.upsert_note(&note)?;

    // Auto-bookmark safety record (best-effort).
    if let Err(e) = inbox_db.create_version(
        &note,
        "absorb_file",
        true,
        Some("Absorbed from file"),
        Some("auto"),
        None,
    ) {
        eprintln!("blot: absorb: could not create auto-bookmark: {e}");
    }

    record_provenance(inbox_db, ef, "inbox_note", &note_id, "", original_action);

    Ok(AbsorbResult {
        target_kind: "inbox_note".to_string(),
        target_id: note_id,
        workspace_path: String::new(),
        title: title.to_string(),
        destination_label: "Inbox".to_string(),
    })
}

// ── Absorb into a workspace ─────────────────────────────────────────────────

/// Absorb an external file into an open workspace at the given destination
/// (Loose Notes / Shelf / Pile). Provenance is recorded in `inbox_db`.
pub fn absorb_into_workspace(
    inbox_db: &InboxDb,
    ws: &WorkspaceDb,
    ef: &ExternalFile,
    title: &str,
    destination: &PlaceDestination,
    original_action: OriginalAction,
) -> Result<AbsorbResult, AbsorbError> {
    let ws_name = ws.workspace_name();
    let ws_path = ws.path.to_string_lossy().into_owned();
    let room_id = destination.room_id();

    let room = ws
        .get_room(room_id)?
        .ok_or_else(|| AbsorbError::DestinationNotFound(format!("Room '{room_id}' not found")))?;

    let now = now_iso8601();
    let doc = document::markdown::parse(&ef.content);
    let document_json = serde_json::to_string(&doc).ok();
    let word_count = ef.content.split_whitespace().count() as i64;

    let ws_note_id = new_id();
    let ws_note = WorkspaceNote {
        id: ws_note_id.clone(),
        title: title.to_string(),
        body: ef.content.clone(),
        document_json,
        auto_titled: false,
        created_at: now.clone(),
        updated_at: now,
        word_count,
        is_archived: false,
    };
    ws.upsert_note(&ws_note)?;

    // Resolve placement + destination label.
    let (shelf_id, destination_label) = match destination {
        PlaceDestination::LooseInRoom { .. } => (None, format!("{ws_name} → {}", room.name)),
        PlaceDestination::InContainer { container_id, .. } => {
            let containers = ws.list_containers_in_room(room_id)?;
            let container = containers
                .iter()
                .find(|c| c.id == *container_id)
                .ok_or_else(|| {
                    AbsorbError::DestinationNotFound(format!(
                        "Container '{container_id}' not found"
                    ))
                })?;
            let label = format!(
                "{ws_name} → {} → {} {}",
                room.name,
                container.kind.display_label(),
                container.name
            );
            (Some(container_id.clone()), label)
        }
    };

    ws.set_note_placement(&NotePlacement {
        note_id: ws_note_id.clone(),
        room_id: room_id.to_string(),
        shelf_id,
        position: 0.0,
    })?;

    // Auto-bookmark safety record (best-effort).
    if let Err(e) = ws.create_note_version(
        &ws_note,
        "absorb_file",
        true,
        Some("Absorbed from file"),
        Some("auto"),
        None,
    ) {
        eprintln!("blot: absorb: could not create workspace auto-bookmark: {e}");
    }

    record_provenance(
        inbox_db,
        ef,
        "workspace_note",
        &ws_note_id,
        &ws_path,
        original_action,
    );

    Ok(AbsorbResult {
        target_kind: "workspace_note".to_string(),
        target_id: ws_note_id,
        workspace_path: ws_path,
        title: title.to_string(),
        destination_label,
    })
}

// ── Duplicate detection ───────────────────────────────────────────────────────

/// Returns `true` if the given source file path has been absorbed before.
pub fn was_absorbed_before(inbox_db: &InboxDb, source_path: &Path) -> bool {
    let key = source_path.to_string_lossy();
    inbox_db
        .find_absorptions_by_source_path(&key)
        .map(|v| !v.is_empty())
        .unwrap_or(false)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn record_provenance(
    inbox_db: &InboxDb,
    ef: &ExternalFile,
    target_kind: &str,
    target_id: &str,
    workspace_path: &str,
    original_action: OriginalAction,
) {
    let rec = AbsorbedFile {
        id: new_note_id(),
        target_kind: target_kind.to_string(),
        target_id: target_id.to_string(),
        workspace_path: workspace_path.to_string(),
        source_file_path: ef.path.to_string_lossy().into_owned(),
        source_file_original_name: ef.original_name.clone(),
        source_file_original_modified_at: ef.original_modified_at.clone(),
        source_file_absorbed_at: now_iso8601(),
        original_action: original_action.as_str().to_string(),
    };
    if let Err(e) = inbox_db.record_absorption(&rec) {
        eprintln!("blot: absorb: could not record provenance: {e}");
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external_file::{ExternalFileKind, LineEnding};
    use crate::workspace::{ContainerKind, WorkspaceDb};
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn open_inbox() -> InboxDb {
        let dir = tempdir().unwrap();
        let db = InboxDb::open(&dir.path().join("inbox.db")).unwrap();
        std::mem::forget(dir);
        db
    }

    fn open_ws(name: &str) -> (WorkspaceDb, PathBuf, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let path = dir.path().join(format!("{name}.water"));
        let ws = WorkspaceDb::create_new(&path, name).unwrap();
        (ws, path, dir)
    }

    fn make_ef(path: &str, content: &str) -> ExternalFile {
        let p = PathBuf::from(path);
        ExternalFile {
            path: p.clone(),
            kind: ExternalFileKind::Markdown,
            content: content.to_string(),
            original_name: p
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            stem: p
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            line_ending: LineEnding::Lf,
            had_trailing_newline: true,
            original_modified_at: Some("2026-01-01T00:00:00Z".to_string()),
            mtime_snapshot: None,
            size_bytes: content.len() as u64,
        }
    }

    fn default_room(ws: &WorkspaceDb) -> String {
        ws.list_rooms().unwrap().into_iter().next().unwrap().id
    }

    #[test]
    fn absorb_into_inbox_creates_note() {
        let db = open_inbox();
        let ef = make_ef("/home/u/notes.md", "# Heading\n\nSome body");

        let result = absorb_into_inbox(&db, &ef, "My File Note", OriginalAction::Leave).unwrap();

        assert_eq!(result.target_kind, "inbox_note");
        assert_eq!(result.destination_label, "Inbox");

        let note = db.get_note(&result.target_id).unwrap().unwrap();
        assert_eq!(note.title, "My File Note");
        assert_eq!(note.body, "# Heading\n\nSome body");
        assert!(!note.auto_titled);
    }

    #[test]
    fn absorb_into_inbox_generates_document_json() {
        let db = open_inbox();
        let ef = make_ef("/home/u/a.md", "# H1\n\nbody text");
        let result = absorb_into_inbox(&db, &ef, "H1", OriginalAction::Leave).unwrap();
        let note = db.get_note(&result.target_id).unwrap().unwrap();
        assert!(note.document_json.is_some());
        assert!(note.document_json.unwrap().contains("heading"));
    }

    #[test]
    fn absorb_into_inbox_creates_auto_bookmark() {
        let db = open_inbox();
        let ef = make_ef("/home/u/a.txt", "content");
        let result = absorb_into_inbox(&db, &ef, "A", OriginalAction::Leave).unwrap();
        let versions = db.list_versions(&result.target_id).unwrap();
        assert_eq!(versions.len(), 1);
        assert!(versions[0].is_bookmark);
        assert_eq!(
            versions[0].bookmark_name.as_deref(),
            Some("Absorbed from file")
        );
        assert_eq!(versions[0].reason, "absorb_file");
    }

    #[test]
    fn absorb_into_inbox_records_provenance() {
        let db = open_inbox();
        let ef = make_ef("/home/u/origin.txt", "content");
        absorb_into_inbox(&db, &ef, "Origin", OriginalAction::Leave).unwrap();

        let found = db
            .find_absorptions_by_source_path("/home/u/origin.txt")
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].source_file_original_name, "origin.txt");
        assert_eq!(
            found[0].source_file_original_modified_at.as_deref(),
            Some("2026-01-01T00:00:00Z")
        );
        assert_eq!(found[0].original_action, "left");
    }

    #[test]
    fn absorb_records_trash_action() {
        let db = open_inbox();
        let ef = make_ef("/home/u/t.txt", "content");
        absorb_into_inbox(&db, &ef, "T", OriginalAction::Trash).unwrap();
        let found = db.find_absorptions_by_source_path("/home/u/t.txt").unwrap();
        assert_eq!(found[0].original_action, "trashed");
    }

    #[test]
    fn absorb_into_workspace_loose_notes() {
        let db = open_inbox();
        let (ws, _path, _dir) = open_ws("TestWS");
        let room_id = default_room(&ws);
        let ef = make_ef("/home/u/w.md", "# Workspace File\n\nbody");

        let result = absorb_into_workspace(
            &db,
            &ws,
            &ef,
            "Workspace File",
            &PlaceDestination::LooseInRoom {
                room_id: room_id.clone(),
            },
            OriginalAction::Leave,
        )
        .unwrap();

        assert_eq!(result.target_kind, "workspace_note");
        assert!(result.destination_label.contains("TestWS"));

        let note = ws.get_note(&result.target_id).unwrap().unwrap();
        assert_eq!(note.title, "Workspace File");
        assert!(note.document_json.is_some());

        let loose = ws.list_loose_notes(&room_id).unwrap();
        assert_eq!(loose.len(), 1);

        // Provenance recorded in inbox DB with workspace path.
        let found = db.find_absorptions_by_source_path("/home/u/w.md").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].target_kind, "workspace_note");
        assert!(!found[0].workspace_path.is_empty());
    }

    #[test]
    fn absorb_into_workspace_shelf() {
        let db = open_inbox();
        let (ws, _path, _dir) = open_ws("TestWS");
        let room_id = default_room(&ws);
        let shelf = ws
            .create_container(&room_id, "Research", ContainerKind::Shelf)
            .unwrap();
        let ef = make_ef("/home/u/s.md", "shelf content");

        let result = absorb_into_workspace(
            &db,
            &ws,
            &ef,
            "Shelf Note",
            &PlaceDestination::InContainer {
                room_id: room_id.clone(),
                container_id: shelf.id.clone(),
            },
            OriginalAction::Leave,
        )
        .unwrap();

        assert_eq!(ws.container_note_count(&shelf.id), 1);
        assert!(result.destination_label.contains("Research"));
        assert!(result.destination_label.contains("Shelf"));
    }

    #[test]
    fn absorb_into_workspace_creates_auto_bookmark() {
        let db = open_inbox();
        let (ws, _path, _dir) = open_ws("TestWS");
        let room_id = default_room(&ws);
        let ef = make_ef("/home/u/b.md", "content");

        let result = absorb_into_workspace(
            &db,
            &ws,
            &ef,
            "B",
            &PlaceDestination::LooseInRoom { room_id },
            OriginalAction::Leave,
        )
        .unwrap();

        let versions = ws.list_note_versions(&result.target_id).unwrap();
        assert_eq!(versions.len(), 1);
        assert!(versions[0].is_bookmark);
    }

    #[test]
    fn absorb_into_workspace_bad_room_errors_and_records_nothing() {
        let db = open_inbox();
        let (ws, _path, _dir) = open_ws("TestWS");
        let ef = make_ef("/home/u/bad.md", "content");

        let result = absorb_into_workspace(
            &db,
            &ws,
            &ef,
            "Bad",
            &PlaceDestination::LooseInRoom {
                room_id: "nonexistent".to_string(),
            },
            OriginalAction::Leave,
        );

        assert!(result.is_err());
        // No provenance recorded for a failed absorb.
        assert!(db
            .find_absorptions_by_source_path("/home/u/bad.md")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn was_absorbed_before_detects_duplicate() {
        let db = open_inbox();
        let ef = make_ef("/home/u/dup.txt", "content");
        assert!(!was_absorbed_before(&db, &ef.path));
        absorb_into_inbox(&db, &ef, "Dup", OriginalAction::Leave).unwrap();
        assert!(was_absorbed_before(&db, &ef.path));
    }
}
