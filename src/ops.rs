//! Pure Rust note operations: Split, Merge, Restore.
//! No GTK dependency — all logic is fully testable.

use crate::inbox::{new_note_id, now_iso8601, InboxDb, InboxNote};
use crate::note_version::NoteVersion;
use crate::workspace::{WorkspaceDb, WorkspaceNote};
use std::fmt;

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum OpsError {
    Inbox(rusqlite::Error),
    Workspace(crate::workspace::WorkspaceError),
    NotFound(String),
    EmptySelection,
}

impl fmt::Display for OpsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inbox(e) => write!(f, "inbox DB error: {e}"),
            Self::Workspace(e) => write!(f, "workspace error: {e}"),
            Self::NotFound(s) => write!(f, "not found: {s}"),
            Self::EmptySelection => write!(f, "selected text is empty"),
        }
    }
}

impl From<rusqlite::Error> for OpsError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Inbox(e)
    }
}

impl From<crate::workspace::WorkspaceError> for OpsError {
    fn from(e: crate::workspace::WorkspaceError) -> Self {
        Self::Workspace(e)
    }
}

// ── Split results ─────────────────────────────────────────────────────────────

pub struct InboxSplitResult {
    /// The new note created from the selected text.
    pub new_note: InboxNote,
    /// Pre-split version of the original note.
    pub version: NoteVersion,
    /// Updated body of the original note (selection replaced with link).
    pub updated_original_body: String,
}

pub struct WorkspaceSplitResult {
    pub new_note: WorkspaceNote,
    pub version: NoteVersion,
    pub updated_original_body: String,
}

// ── Split ─────────────────────────────────────────────────────────────────────

/// Split an inbox note: auto-bookmark, create new note from `selected_text`,
/// replace selection in original with `[[new_title]]`.
pub fn split_inbox_note(
    db: &InboxDb,
    note: &InboxNote,
    selected_text: &str,
) -> Result<InboxSplitResult, OpsError> {
    if selected_text.trim().is_empty() {
        return Err(OpsError::EmptySelection);
    }

    // Auto-bookmark original before modifying.
    let version = db.create_version(note, "before split", false, None, Some("auto"), None)?;

    // Derive a title for the new note from the first line of the selection.
    let new_title = derive_title_from_text(selected_text);

    let new_id = new_note_id();
    let now = now_iso8601();
    let new_note = InboxNote {
        id: new_id.clone(),
        title: new_title.clone(),
        body: selected_text.to_string(),
        document_json: None,
        auto_titled: true,
        created_at: now.clone(),
        updated_at: now,
        word_count: word_count(selected_text),
        is_pinned: false,
        is_archived: false,
        placed_at: None,
        placed_workspace_path: None,
        placed_workspace_note_id: None,
        placed_destination_label: None,
    };
    db.upsert_note(&new_note)?;

    // Replace the selected text in the original with a link.
    let link = format!("[[{new_title}]]");
    let updated_body = note.body.replacen(selected_text, &link, 1);

    Ok(InboxSplitResult {
        new_note,
        version,
        updated_original_body: updated_body,
    })
}

/// Split a workspace note. Creates the new note as loose in the same room.
pub fn split_workspace_note(
    db: &WorkspaceDb,
    note: &WorkspaceNote,
    room_id: &str,
    selected_text: &str,
) -> Result<WorkspaceSplitResult, OpsError> {
    if selected_text.trim().is_empty() {
        return Err(OpsError::EmptySelection);
    }

    let version = db.create_note_version(note, "before split", false, None, Some("auto"), None)?;

    let new_title = derive_title_from_text(selected_text);

    let new_id = crate::workspace::new_id();
    let now = crate::workspace::now_iso8601();
    let new_note = WorkspaceNote {
        id: new_id.clone(),
        title: new_title.clone(),
        body: selected_text.to_string(),
        document_json: None,
        auto_titled: true,
        created_at: now.clone(),
        updated_at: now,
        word_count: word_count(selected_text),
        is_archived: false,
    };
    db.upsert_note(&new_note)?;
    db.set_note_placement(&crate::workspace::NotePlacement {
        note_id: new_id,
        room_id: room_id.to_string(),
        shelf_id: None,
        position: 0.0,
    })?;

    let link = format!("[[{new_title}]]");
    let updated_body = note.body.replacen(selected_text, &link, 1);

    Ok(WorkspaceSplitResult {
        new_note,
        version,
        updated_original_body: updated_body,
    })
}

// ── Merge ─────────────────────────────────────────────────────────────────────

/// Merge inbox notes: auto-bookmark all, append sources as sections into target,
/// archive sources.  Returns updated body for the target note.
pub fn merge_inbox_notes(
    db: &InboxDb,
    target: &InboxNote,
    sources: &[&InboxNote],
    operation_id: &str,
) -> Result<String, OpsError> {
    if sources.is_empty() {
        return Ok(target.body.clone());
    }

    // Bookmark target and all sources before modifying anything.
    db.create_version(
        target,
        "before merge (target)",
        false,
        None,
        Some("auto"),
        Some(operation_id),
    )?;
    for source in sources {
        db.create_version(
            source,
            "before merge (source)",
            false,
            None,
            Some("auto"),
            Some(operation_id),
        )?;
    }

    // Build merged body.
    let mut merged = target.body.clone();
    for source in sources {
        let section = format!("\n\n## Merged from: {}\n\n{}", source.title, source.body);
        merged.push_str(&section);
    }

    // Archive source notes, and migrate any pin from a source onto the target
    // so merging never leaves a broken pin pointing at an archived note.
    let mut target_should_pin = false;
    for source in sources {
        if db.is_note_pinned("inbox_note", &source.id, "") {
            db.unpin_note("inbox_note", &source.id, "")?;
            target_should_pin = true;
        }
        db.archive_as_merged(&source.id, "inbox_note", &target.id, "")?;
    }
    if target_should_pin && !db.is_note_pinned("inbox_note", &target.id, "") {
        let snippet = target.body.chars().take(80).collect::<String>();
        db.pin_note("inbox_note", &target.id, "", &target.title, &snippet)?;
    }

    Ok(merged)
}

/// Merge workspace notes into a target. Returns updated body.
pub fn merge_workspace_notes(
    db: &WorkspaceDb,
    target: &WorkspaceNote,
    sources: &[&WorkspaceNote],
    operation_id: &str,
) -> Result<String, OpsError> {
    if sources.is_empty() {
        return Ok(target.body.clone());
    }

    db.create_note_version(
        target,
        "before merge (target)",
        false,
        None,
        Some("auto"),
        Some(operation_id),
    )?;
    for source in sources {
        db.create_note_version(
            source,
            "before merge (source)",
            false,
            None,
            Some("auto"),
            Some(operation_id),
        )?;
    }

    let mut merged = target.body.clone();
    for source in sources {
        let section = format!("\n\n## Merged from: {}\n\n{}", source.title, source.body);
        merged.push_str(&section);
    }

    for source in sources {
        db.archive_as_merged(&source.id, &target.id)?;
    }

    Ok(merged)
}

// ── Restore ───────────────────────────────────────────────────────────────────

/// Apply a version snapshot to an inbox note: returns the updated InboxNote.
/// The caller is responsible for calling `db.upsert_note(&updated)` and
/// refreshing the editor.
pub fn restore_inbox_version(db: &InboxDb, version: &NoteVersion) -> Result<InboxNote, OpsError> {
    let existing = db
        .get_note(&version.note_id)?
        .ok_or_else(|| OpsError::NotFound(version.note_id.clone()))?;

    // Bookmark current state before restoring.
    db.create_version(&existing, "before restore", false, None, Some("auto"), None)?;

    let now = now_iso8601();
    let restored = InboxNote {
        title: version.title.clone(),
        body: version.body.clone(),
        document_json: version.document_json.clone(),
        auto_titled: existing.auto_titled,
        updated_at: now,
        ..existing
    };
    db.upsert_note(&restored)?;
    Ok(restored)
}

/// Apply a version snapshot to a workspace note. Returns the updated WorkspaceNote.
pub fn restore_workspace_version(
    db: &WorkspaceDb,
    version: &NoteVersion,
) -> Result<WorkspaceNote, OpsError> {
    let existing = db
        .get_note(&version.note_id)?
        .ok_or_else(|| OpsError::NotFound(version.note_id.clone()))?;

    db.create_note_version(&existing, "before restore", false, None, Some("auto"), None)?;

    let now = crate::workspace::now_iso8601();
    let restored = WorkspaceNote {
        title: version.title.clone(),
        body: version.body.clone(),
        document_json: version.document_json.clone(),
        auto_titled: existing.auto_titled,
        updated_at: now,
        ..existing
    };
    db.upsert_note(&restored)?;
    Ok(restored)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Smart title for a note created from selected text. Reuses the canonical
/// smart-title logic (UTF-8 safe, heading-aware) and falls back to a generic
/// label when the selection has no meaningful first line.
fn derive_title_from_text(text: &str) -> String {
    let title = crate::title::derive_title(text);
    if title.is_empty() {
        "Untitled note".to_string()
    } else {
        title
    }
}

fn word_count(text: &str) -> i64 {
    text.split_whitespace().count() as i64
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inbox::InboxDb;
    use crate::workspace::{ContainerKind, WorkspaceDb};
    use tempfile::tempdir;

    fn open_inbox() -> InboxDb {
        let dir = tempdir().unwrap();
        let path = dir.path().join("inbox.db");
        let db = InboxDb::open(&path).unwrap();
        std::mem::forget(dir);
        db
    }

    fn open_workspace() -> (WorkspaceDb, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.water");
        let ws = WorkspaceDb::create_new(&path, "Test").unwrap();
        (ws, dir)
    }

    fn make_inbox_note(db: &InboxDb, id: &str, title: &str, body: &str) -> InboxNote {
        let note = InboxNote {
            id: id.to_string(),
            title: title.to_string(),
            body: body.to_string(),
            document_json: None,
            auto_titled: true,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            word_count: body.split_whitespace().count() as i64,
            is_pinned: false,
            is_archived: false,
            placed_at: None,
            placed_workspace_path: None,
            placed_workspace_note_id: None,
            placed_destination_label: None,
        };
        db.upsert_note(&note).unwrap();
        note
    }

    fn make_ws_note(ws: &WorkspaceDb, title: &str, body: &str) -> WorkspaceNote {
        let room = ws.list_rooms().unwrap().into_iter().next().unwrap();
        let mut note = ws.create_loose_note(&room.id).unwrap();
        note.title = title.to_string();
        note.body = body.to_string();
        ws.upsert_note(&note).unwrap();
        note
    }

    // ── Split inbox ───────────────────────────────────────────────────────────

    #[test]
    fn split_inbox_creates_new_note_and_link() {
        let db = open_inbox();
        let note = make_inbox_note(&db, "n1", "Main", "Hello world\nExtra content");

        let result = split_inbox_note(&db, &note, "Hello world").unwrap();

        assert_eq!(result.new_note.body, "Hello world");
        assert!(result.updated_original_body.contains("[["));
        assert!(!result.updated_original_body.contains("Hello world\n"));
        // Auto-bookmark was created.
        assert_eq!(db.list_versions("n1").unwrap().len(), 1);
    }

    #[test]
    fn split_inbox_empty_selection_errors() {
        let db = open_inbox();
        let note = make_inbox_note(&db, "n1", "Main", "Hello");
        assert!(matches!(
            split_inbox_note(&db, &note, "   "),
            Err(OpsError::EmptySelection)
        ));
    }

    #[test]
    fn split_inbox_new_note_is_saved() {
        let db = open_inbox();
        let note = make_inbox_note(&db, "n1", "Main", "Hello world");
        let result = split_inbox_note(&db, &note, "Hello world").unwrap();

        let fetched = db.get_note(&result.new_note.id).unwrap();
        assert!(fetched.is_some());
    }

    // ── Split workspace ───────────────────────────────────────────────────────

    #[test]
    fn split_workspace_creates_new_note_and_link() {
        let (ws, _dir) = open_workspace();
        let note = make_ws_note(&ws, "Main", "Introduction text here");
        let room = ws.list_rooms().unwrap().into_iter().next().unwrap();

        let result = split_workspace_note(&ws, &note, &room.id, "Introduction text").unwrap();

        assert_eq!(result.new_note.body, "Introduction text");
        assert!(result.updated_original_body.contains("[["));
        assert_eq!(ws.list_note_versions(&note.id).unwrap().len(), 1);
    }

    // ── Merge inbox ───────────────────────────────────────────────────────────

    #[test]
    fn merge_inbox_appends_sections_and_archives_sources() {
        let db = open_inbox();
        let target = make_inbox_note(&db, "t1", "Target", "Target body");
        let src1 = make_inbox_note(&db, "s1", "Source A", "Content A");
        let src2 = make_inbox_note(&db, "s2", "Source B", "Content B");

        let merged = merge_inbox_notes(&db, &target, &[&src1, &src2], "op-1").unwrap();

        assert!(merged.contains("## Merged from: Source A"));
        assert!(merged.contains("Content A"));
        assert!(merged.contains("## Merged from: Source B"));

        // Sources should be archived.
        assert!(db.get_note("s1").unwrap().unwrap().is_archived);
        assert!(db.get_note("s2").unwrap().unwrap().is_archived);

        // Versions were created for target + both sources.
        assert_eq!(db.list_versions("t1").unwrap().len(), 1);
        assert_eq!(db.list_versions("s1").unwrap().len(), 1);
    }

    #[test]
    fn merge_inbox_pin_follows_to_target() {
        let db = open_inbox();
        let target = make_inbox_note(&db, "t1", "Target", "Target body");
        let src = make_inbox_note(&db, "s1", "Source", "Source content");
        db.pin_note("inbox_note", "s1", "", "Source", "Source content")
            .unwrap();

        merge_inbox_notes(&db, &target, &[&src], "op-pin").unwrap();

        // Source pin removed, target now pinned.
        assert!(!db.is_note_pinned("inbox_note", "s1", ""));
        assert!(db.is_note_pinned("inbox_note", "t1", ""));
    }

    #[test]
    fn merge_inbox_empty_sources_returns_original_body() {
        let db = open_inbox();
        let target = make_inbox_note(&db, "t1", "Target", "Original body");
        let merged = merge_inbox_notes(&db, &target, &[], "op-x").unwrap();
        assert_eq!(merged, "Original body");
    }

    // ── Merge workspace ───────────────────────────────────────────────────────

    #[test]
    fn merge_workspace_appends_sections_and_archives_sources() {
        let (ws, _dir) = open_workspace();
        let target = make_ws_note(&ws, "Target", "Target body");
        let src = make_ws_note(&ws, "Source", "Source content");

        let merged = merge_workspace_notes(&ws, &target, &[&src], "op-2").unwrap();

        assert!(merged.contains("## Merged from: Source"));
        let src_loaded = ws.get_note(&src.id).unwrap().unwrap();
        assert!(src_loaded.is_archived);
    }

    // ── Restore inbox ─────────────────────────────────────────────────────────

    #[test]
    fn restore_inbox_version_reverts_content() {
        let db = open_inbox();
        let note = make_inbox_note(&db, "n1", "Original", "Original body");
        let version = db
            .create_version(&note, "snap", false, None, None, None)
            .unwrap();

        // Modify the note.
        let mut modified = note.clone();
        modified.body = "Modified body".to_string();
        modified.title = "Modified".to_string();
        db.upsert_note(&modified).unwrap();

        let restored = restore_inbox_version(&db, &version).unwrap();
        assert_eq!(restored.title, "Original");
        assert_eq!(restored.body, "Original body");

        // A "before restore" version was created.
        let versions = db.list_versions("n1").unwrap();
        assert_eq!(versions.len(), 2);
    }

    // ── Restore workspace ─────────────────────────────────────────────────────

    #[test]
    fn restore_workspace_version_reverts_content() {
        let (ws, _dir) = open_workspace();
        let note = make_ws_note(&ws, "Original", "Original body");
        let version = ws
            .create_note_version(&note, "snap", false, None, None, None)
            .unwrap();

        let mut modified = note.clone();
        modified.body = "Changed".to_string();
        ws.upsert_note(&modified).unwrap();

        let restored = restore_workspace_version(&ws, &version).unwrap();
        assert_eq!(restored.body, "Original body");
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    #[test]
    fn derive_title_from_heading() {
        assert_eq!(derive_title_from_text("# My Heading\nbody"), "My Heading");
    }

    #[test]
    fn derive_title_empty_falls_back() {
        assert_eq!(derive_title_from_text("   \n"), "Untitled note");
    }

    #[test]
    fn derive_title_truncates_long_line() {
        let long = "a".repeat(120);
        let title = derive_title_from_text(&long);
        // Smart-title caps at 80 chars + ellipsis.
        assert!(title.chars().count() <= 81);
        assert!(title.ends_with('…'));
    }

    #[test]
    fn split_with_workspace_pile_container() {
        let (ws, _dir) = open_workspace();
        let room = ws.list_rooms().unwrap().into_iter().next().unwrap();
        ws.create_container(&room.id, "Drafts", ContainerKind::Pile)
            .unwrap();
        let note = make_ws_note(&ws, "Long Note", "First part\nSecond part");

        let result = split_workspace_note(&ws, &note, &room.id, "First part").unwrap();
        assert!(result.updated_original_body.contains("[["));
    }
}
