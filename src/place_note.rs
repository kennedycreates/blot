//! Place Note — moves an Inbox note into a .water workspace.
//!
//! The Inbox is a chaos quarantine zone. Place Note is how a thought becomes
//! part of the user's organized Watercolor world.
//!
//! Placement is transactional:
//! 1. Validate the inbox note exists and is not already placed.
//! 2. Open the destination workspace.
//! 3. Insert a workspace note with all content preserved.
//! 4. Set the note placement (room / shelf / pile / loose).
//! 5. Archive the inbox note with placement metadata.
//! 6. Transfer the global pin if the note was pinned.
//! 7. Record the placed workspace note in recents.
//!
//! Safety contract:
//! - If workspace insert fails → inbox note stays active. No side effects.
//! - If inbox archive fails after a successful workspace insert → return
//!   `InboxArchiveIncomplete`. The workspace note exists. The caller must warn
//!   the user. Do not delete anything silently.

use crate::inbox::{new_note_id, now_iso8601, InboxDb, InboxNote, RecentEntry};
use crate::known_workspaces::KnownWorkspaceRegistry;
use crate::workspace::{new_id, ContainerKind, NotePlacement, WorkspaceDb, WorkspaceNote};
use std::fmt;
use std::path::{Path, PathBuf};

// ── Types ─────────────────────────────────────────────────────────────────────

/// Where in a workspace to place the note.
#[derive(Debug, Clone)]
pub enum PlaceDestination {
    /// Place loose in a Room — not on any shelf or pile.
    LooseInRoom { room_id: String },
    /// Place on a Shelf or in a Pile.
    InContainer {
        room_id: String,
        container_id: String,
    },
}

impl PlaceDestination {
    pub fn room_id(&self) -> &str {
        match self {
            Self::LooseInRoom { room_id } => room_id,
            Self::InContainer { room_id, .. } => room_id,
        }
    }
}

/// The full request to place an inbox note.
#[derive(Debug, Clone)]
pub struct PlacementRequest {
    pub inbox_note_id: String,
    pub workspace_path: PathBuf,
    pub destination: PlaceDestination,
}

/// Returned on successful placement.
#[derive(Debug, Clone)]
pub struct PlacementResult {
    pub workspace_note_id: String,
    pub workspace_path: PathBuf,
    pub workspace_name: String,
    pub destination_label: String,
}

/// All possible placement failures.
#[derive(Debug)]
pub enum PlacementError {
    InboxNoteNotFound,
    InboxNoteAlreadyPlaced,
    WorkspaceOpenFailed(String),
    WorkspaceInsertFailed(String),
    /// Workspace note was created, but inbox archive failed.
    /// The workspace note exists; the inbox note also still exists.
    /// Caller must warn the user and provide a repair path.
    InboxArchiveIncomplete {
        workspace_note_id: String,
        workspace_path: PathBuf,
        destination_label: String,
        error: String,
    },
    DestinationNotFound(String),
    InboxDbUnavailable,
}

impl fmt::Display for PlacementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InboxNoteNotFound => write!(f, "Inbox note not found."),
            Self::InboxNoteAlreadyPlaced => {
                write!(f, "This note has already been placed into a workspace.")
            }
            Self::WorkspaceOpenFailed(e) => write!(f, "Could not open the workspace: {e}"),
            Self::WorkspaceInsertFailed(e) => write!(f, "Could not write note to workspace: {e}"),
            Self::InboxArchiveIncomplete {
                destination_label,
                error,
                workspace_note_id,
                ..
            } => write!(
                f,
                "Note was placed in {destination_label} but could not be removed from Inbox: \
                 {error}. The workspace note ({workspace_note_id}) is safe. \
                 Please remove the Inbox note manually to avoid a duplicate."
            ),
            Self::DestinationNotFound(msg) => write!(f, "Destination not found: {msg}"),
            Self::InboxDbUnavailable => write!(f, "Inbox database is not available."),
        }
    }
}

// ── Main transaction ──────────────────────────────────────────────────────────

/// Move an Inbox note into a .water workspace.
///
/// Returns the placed note's info on success. On any failure before the
/// workspace note is safely written, the inbox note remains intact.
pub fn place_inbox_note(
    inbox_db: &InboxDb,
    request: &PlacementRequest,
) -> Result<PlacementResult, PlacementError> {
    // Step 1 — fetch and validate the inbox note.
    let inbox_note = inbox_db
        .get_note(&request.inbox_note_id)
        .map_err(|_| PlacementError::InboxDbUnavailable)?
        .ok_or(PlacementError::InboxNoteNotFound)?;

    if inbox_note.is_archived {
        return Err(PlacementError::InboxNoteAlreadyPlaced);
    }

    // Step 2 — open the destination workspace.
    let ws = WorkspaceDb::open(&request.workspace_path)
        .map_err(|e| PlacementError::WorkspaceOpenFailed(e.to_string()))?;

    let ws_name = ws.workspace_name();

    // Step 3 — validate destination room exists.
    let room_id = request.destination.room_id();
    let room = ws
        .get_room(room_id)
        .map_err(|e| PlacementError::WorkspaceInsertFailed(e.to_string()))?
        .ok_or_else(|| {
            PlacementError::DestinationNotFound(format!("Room '{room_id}' not found"))
        })?;

    // Step 4 — build the workspace note, preserving all content.
    let now = now_iso8601();
    let ws_note_id = new_id();
    let ws_note = WorkspaceNote {
        id: ws_note_id.clone(),
        title: inbox_note.title.clone(),
        body: inbox_note.body.clone(),
        document_json: inbox_note.document_json.clone(),
        auto_titled: inbox_note.auto_titled,
        created_at: inbox_note.created_at.clone(),
        updated_at: now.clone(),
        word_count: inbox_note.word_count,
        is_archived: false,
    };

    // Step 5 — insert into workspace.
    ws.upsert_note(&ws_note)
        .map_err(|e| PlacementError::WorkspaceInsertFailed(e.to_string()))?;

    // Step 6 — set placement and build the destination label.
    let (shelf_id, destination_label) = match &request.destination {
        PlaceDestination::LooseInRoom { .. } => (None, format!("{ws_name} → {}", room.name)),
        PlaceDestination::InContainer { container_id, .. } => {
            let containers = ws
                .list_containers_in_room(room_id)
                .map_err(|e| PlacementError::WorkspaceInsertFailed(e.to_string()))?;
            let container = containers
                .iter()
                .find(|c| c.id == *container_id)
                .ok_or_else(|| {
                    PlacementError::DestinationNotFound(format!(
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
    })
    .map_err(|e| PlacementError::WorkspaceInsertFailed(e.to_string()))?;

    // Step 7 — archive the inbox note.
    // If this fails, we have a partial placement. Return InboxArchiveIncomplete
    // rather than silently deleting anything.
    let archive_result = inbox_db.mark_as_placed(
        &request.inbox_note_id,
        &request.workspace_path.to_string_lossy(),
        &ws_note_id,
        &destination_label,
    );

    if let Err(e) = archive_result {
        return Err(PlacementError::InboxArchiveIncomplete {
            workspace_note_id: ws_note_id,
            workspace_path: request.workspace_path.clone(),
            destination_label,
            error: e.to_string(),
        });
    }

    // Step 8 — transfer global pin if the note was pinned.
    if inbox_note.is_pinned {
        if let Err(e) = transfer_pin(inbox_db, &inbox_note, &ws_note_id, &request.workspace_path) {
            eprintln!("blot: warning: could not transfer pin after placement: {e}");
        }
    }

    // Step 9 — record the placed workspace note in recents.
    let snippet: String = inbox_note
        .body
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(80)
        .collect();

    let _ = inbox_db.touch_recent(&RecentEntry {
        id: new_note_id(),
        target_kind: "workspace_note".to_string(),
        target_id: ws_note_id.clone(),
        workspace_path: request.workspace_path.to_string_lossy().into_owned(),
        workspace_name: ws_name.clone(),
        note_title: inbox_note.title.clone(),
        note_snippet: snippet,
        accessed_at: now,
    });

    Ok(PlacementResult {
        workspace_note_id: ws_note_id,
        workspace_path: request.workspace_path.clone(),
        workspace_name: ws_name,
        destination_label,
    })
}

// ── Pin transfer ──────────────────────────────────────────────────────────────

fn transfer_pin(
    inbox_db: &InboxDb,
    inbox_note: &InboxNote,
    ws_note_id: &str,
    ws_path: &Path,
) -> rusqlite::Result<()> {
    let ws_path_str = ws_path.to_string_lossy();
    inbox_db.unpin_note("inbox_note", &inbox_note.id, "")?;
    let snippet: String = inbox_note
        .body
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(80)
        .collect();
    inbox_db.pin_note(
        "workspace_note",
        ws_note_id,
        &ws_path_str,
        &inbox_note.title,
        &snippet,
    )?;
    Ok(())
}

// ── Suggestion heuristics ─────────────────────────────────────────────────────

/// A suggested placement destination for the picker to pre-populate.
#[derive(Debug, Clone)]
pub struct PlacementSuggestion {
    pub workspace_path: PathBuf,
    pub workspace_name: String,
    /// Pre-suggested room ID (last used, or default room). None = let dialog pick first.
    pub room_id: Option<String>,
    /// Pre-suggested container ID for Shelf/Pile. None = suggest Loose Notes.
    pub container_id: Option<String>,
    /// Kind of the suggested container.
    pub container_kind: Option<ContainerKind>,
}

/// Compute a simple placement suggestion.
///
/// Priority:
/// 1. Currently focused workspace (if any).
/// 2. Last-used workspace from the registry.
/// 3. Within that workspace, last-used room and container.
/// 4. Fall back to Loose Notes in the default room.
pub fn compute_suggestion(
    focused_workspace: Option<&WorkspaceDb>,
    known_workspaces: &KnownWorkspaceRegistry,
) -> Option<PlacementSuggestion> {
    let (ws_path, ws_name) = if let Some(ws) = focused_workspace {
        (ws.path.clone(), ws.workspace_name())
    } else {
        let first = known_workspaces.list().first()?;
        (first.path.clone(), first.display_name.clone())
    };

    let entry = known_workspaces.list().iter().find(|w| w.path == ws_path);
    let room_id = entry.and_then(|e| e.last_room_id.clone());
    let container_id = entry.and_then(|e| e.last_container_id.clone());
    let container_kind = entry
        .and_then(|e| e.last_container_kind.as_deref())
        .map(ContainerKind::from_str);

    Some(PlacementSuggestion {
        workspace_path: ws_path,
        workspace_name: ws_name,
        room_id,
        container_id,
        container_kind,
    })
}

/// Update the known-workspaces registry with the last-used placement destination.
/// Called after a successful placement.
pub fn record_last_used_destination(
    known_workspaces: &mut KnownWorkspaceRegistry,
    workspace_path: &Path,
    room_id: &str,
    container_id: Option<&str>,
    container_kind: Option<&str>,
) {
    let now = now_iso8601();
    if let Some(entry) = known_workspaces
        .workspaces
        .iter_mut()
        .find(|w| w.path == workspace_path)
    {
        entry.last_room_id = Some(room_id.to_string());
        entry.last_container_id = container_id.map(str::to_string);
        entry.last_container_kind = container_kind.map(str::to_string);
        entry.last_focused_at = now;
    }
    known_workspaces.save();
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inbox::{InboxDb, InboxNote};
    use crate::workspace::{ContainerKind, WorkspaceDb};
    use tempfile::tempdir;

    fn make_inbox_db(dir: &tempfile::TempDir) -> InboxDb {
        InboxDb::open(&dir.path().join("inbox.db")).unwrap()
    }

    fn make_workspace(dir: &tempfile::TempDir, name: &str) -> (WorkspaceDb, PathBuf) {
        let path = dir.path().join(format!("{name}.water"));
        let ws = WorkspaceDb::create_new(&path, name).unwrap();
        (ws, path)
    }

    fn make_inbox_note(db: &InboxDb, title: &str, body: &str) -> InboxNote {
        let note = InboxNote {
            id: new_note_id(),
            title: title.to_string(),
            body: body.to_string(),
            document_json: Some(r#"{"schema_version":1,"blocks":[]}"#.to_string()),
            auto_titled: false,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            word_count: 3,
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

    fn default_room_id(ws: &WorkspaceDb) -> String {
        ws.list_rooms().unwrap().into_iter().next().unwrap().id
    }

    // ── Place to Loose Notes ──────────────────────────────────────────────────

    #[test]
    fn place_to_loose_notes() {
        let dir = tempdir().unwrap();
        let inbox = make_inbox_db(&dir);
        let (_ws, ws_path) = make_workspace(&dir, "TestWS");
        let note = make_inbox_note(&inbox, "My Note", "Hello world");
        let ws = WorkspaceDb::open(&ws_path).unwrap();
        let room_id = default_room_id(&ws);

        let result = place_inbox_note(
            &inbox,
            &PlacementRequest {
                inbox_note_id: note.id.clone(),
                workspace_path: ws_path.clone(),
                destination: PlaceDestination::LooseInRoom {
                    room_id: room_id.clone(),
                },
            },
        )
        .unwrap();

        assert_eq!(result.workspace_path, ws_path);
        assert!(!result.workspace_note_id.is_empty());
        assert!(result.destination_label.contains("TestWS"));

        let ws2 = WorkspaceDb::open(&ws_path).unwrap();
        let ws_note = ws2.get_note(&result.workspace_note_id).unwrap().unwrap();
        assert_eq!(ws_note.title, "My Note");
        assert_eq!(ws_note.body, "Hello world");

        let loose = ws2.list_loose_notes(&room_id).unwrap();
        assert_eq!(loose.len(), 1);
    }

    // ── Place to Shelf ────────────────────────────────────────────────────────

    #[test]
    fn place_to_shelf() {
        let dir = tempdir().unwrap();
        let inbox = make_inbox_db(&dir);
        let (ws, ws_path) = make_workspace(&dir, "TestWS");
        let note = make_inbox_note(&inbox, "Shelf Note", "For the shelf");
        let room_id = default_room_id(&ws);
        let shelf = ws
            .create_container(&room_id, "Research", ContainerKind::Shelf)
            .unwrap();

        let result = place_inbox_note(
            &inbox,
            &PlacementRequest {
                inbox_note_id: note.id.clone(),
                workspace_path: ws_path.clone(),
                destination: PlaceDestination::InContainer {
                    room_id: room_id.clone(),
                    container_id: shelf.id.clone(),
                },
            },
        )
        .unwrap();

        let ws2 = WorkspaceDb::open(&ws_path).unwrap();
        assert_eq!(ws2.container_note_count(&shelf.id), 1);
        assert!(result.destination_label.contains("Research"));
        assert!(result.destination_label.contains("Shelf"));
    }

    // ── Place to Pile ─────────────────────────────────────────────────────────

    #[test]
    fn place_to_pile() {
        let dir = tempdir().unwrap();
        let inbox = make_inbox_db(&dir);
        let (ws, ws_path) = make_workspace(&dir, "TestWS");
        let note = make_inbox_note(&inbox, "Pile Note", "For the pile");
        let room_id = default_room_id(&ws);
        let pile = ws
            .create_container(&room_id, "Drafts", ContainerKind::Pile)
            .unwrap();

        let result = place_inbox_note(
            &inbox,
            &PlacementRequest {
                inbox_note_id: note.id.clone(),
                workspace_path: ws_path.clone(),
                destination: PlaceDestination::InContainer {
                    room_id: room_id.clone(),
                    container_id: pile.id.clone(),
                },
            },
        )
        .unwrap();

        let ws2 = WorkspaceDb::open(&ws_path).unwrap();
        assert_eq!(ws2.container_note_count(&pile.id), 1);
        assert!(result.destination_label.contains("Drafts"));
        assert!(result.destination_label.contains("Pile"));
    }

    // ── Create Room during placement ──────────────────────────────────────────

    #[test]
    fn create_room_and_place() {
        let dir = tempdir().unwrap();
        let inbox = make_inbox_db(&dir);
        let (ws, ws_path) = make_workspace(&dir, "TestWS");
        let note = make_inbox_note(&inbox, "Note", "Content");
        let new_room = ws.create_room("New Research Room").unwrap();

        let result = place_inbox_note(
            &inbox,
            &PlacementRequest {
                inbox_note_id: note.id.clone(),
                workspace_path: ws_path.clone(),
                destination: PlaceDestination::LooseInRoom {
                    room_id: new_room.id.clone(),
                },
            },
        )
        .unwrap();

        let ws2 = WorkspaceDb::open(&ws_path).unwrap();
        let loose = ws2.list_loose_notes(&new_room.id).unwrap();
        assert_eq!(loose.len(), 1);
        assert!(result.destination_label.contains("New Research Room"));
    }

    // ── Create Shelf during placement ─────────────────────────────────────────

    #[test]
    fn create_shelf_and_place() {
        let dir = tempdir().unwrap();
        let inbox = make_inbox_db(&dir);
        let (ws, ws_path) = make_workspace(&dir, "TestWS");
        let note = make_inbox_note(&inbox, "Note", "Content");
        let room_id = default_room_id(&ws);
        let new_shelf = ws
            .create_container(&room_id, "Brand New Shelf", ContainerKind::Shelf)
            .unwrap();

        let result = place_inbox_note(
            &inbox,
            &PlacementRequest {
                inbox_note_id: note.id.clone(),
                workspace_path: ws_path.clone(),
                destination: PlaceDestination::InContainer {
                    room_id: room_id.clone(),
                    container_id: new_shelf.id.clone(),
                },
            },
        )
        .unwrap();

        let ws2 = WorkspaceDb::open(&ws_path).unwrap();
        assert_eq!(ws2.container_note_count(&new_shelf.id), 1);
        assert!(result.destination_label.contains("Brand New Shelf"));
    }

    // ── Create Pile during placement ──────────────────────────────────────────

    #[test]
    fn create_pile_and_place() {
        let dir = tempdir().unwrap();
        let inbox = make_inbox_db(&dir);
        let (ws, ws_path) = make_workspace(&dir, "TestWS");
        let note = make_inbox_note(&inbox, "Note", "Content");
        let room_id = default_room_id(&ws);
        let new_pile = ws
            .create_container(&room_id, "Ideas Pile", ContainerKind::Pile)
            .unwrap();

        place_inbox_note(
            &inbox,
            &PlacementRequest {
                inbox_note_id: note.id.clone(),
                workspace_path: ws_path.clone(),
                destination: PlaceDestination::InContainer {
                    room_id: room_id.clone(),
                    container_id: new_pile.id.clone(),
                },
            },
        )
        .unwrap();

        let ws2 = WorkspaceDb::open(&ws_path).unwrap();
        assert_eq!(ws2.container_note_count(&new_pile.id), 1);
    }

    // ── Failure: workspace unavailable → inbox note kept ──────────────────────

    #[test]
    fn failed_workspace_open_keeps_inbox_note() {
        let dir = tempdir().unwrap();
        let inbox = make_inbox_db(&dir);
        let note = make_inbox_note(&inbox, "Safe Note", "Content");
        let bad_path = dir.path().join("nonexistent.water");

        let result = place_inbox_note(
            &inbox,
            &PlacementRequest {
                inbox_note_id: note.id.clone(),
                workspace_path: bad_path,
                destination: PlaceDestination::LooseInRoom {
                    room_id: "some-id".to_string(),
                },
            },
        );

        assert!(result.is_err());
        let notes = inbox.list_notes().unwrap();
        assert_eq!(notes.len(), 1);
        assert!(!notes[0].is_archived);
    }

    // ── Failure: bad room ID → inbox note kept ────────────────────────────────

    #[test]
    fn failed_destination_insert_keeps_inbox_note() {
        let dir = tempdir().unwrap();
        let inbox = make_inbox_db(&dir);
        let (_ws, ws_path) = make_workspace(&dir, "TestWS");
        let note = make_inbox_note(&inbox, "Note", "Content");

        let result = place_inbox_note(
            &inbox,
            &PlacementRequest {
                inbox_note_id: note.id.clone(),
                workspace_path: ws_path,
                destination: PlaceDestination::LooseInRoom {
                    room_id: "bad-room-id-that-does-not-exist".to_string(),
                },
            },
        );

        assert!(result.is_err());
        assert_eq!(inbox.list_notes().unwrap().len(), 1);
    }

    // ── Note not found ────────────────────────────────────────────────────────

    #[test]
    fn inbox_note_not_found_returns_error() {
        let dir = tempdir().unwrap();
        let inbox = make_inbox_db(&dir);
        let (_ws, ws_path) = make_workspace(&dir, "TestWS");

        let result = place_inbox_note(
            &inbox,
            &PlacementRequest {
                inbox_note_id: "nonexistent-id".to_string(),
                workspace_path: ws_path,
                destination: PlaceDestination::LooseInRoom {
                    room_id: "room-id".to_string(),
                },
            },
        );

        assert!(matches!(result, Err(PlacementError::InboxNoteNotFound)));
    }

    // ── Double-placement rejected ─────────────────────────────────────────────

    #[test]
    fn already_placed_note_returns_error() {
        let dir = tempdir().unwrap();
        let inbox = make_inbox_db(&dir);
        let (_ws, ws_path) = make_workspace(&dir, "TestWS");
        let note = make_inbox_note(&inbox, "Note", "Content");
        let ws = WorkspaceDb::open(&ws_path).unwrap();
        let room_id = default_room_id(&ws);

        place_inbox_note(
            &inbox,
            &PlacementRequest {
                inbox_note_id: note.id.clone(),
                workspace_path: ws_path.clone(),
                destination: PlaceDestination::LooseInRoom {
                    room_id: room_id.clone(),
                },
            },
        )
        .unwrap();

        let result = place_inbox_note(
            &inbox,
            &PlacementRequest {
                inbox_note_id: note.id.clone(),
                workspace_path: ws_path,
                destination: PlaceDestination::LooseInRoom { room_id },
            },
        );

        assert!(matches!(
            result,
            Err(PlacementError::InboxNoteAlreadyPlaced)
        ));
    }

    // ── Placed note hidden from active Inbox list ─────────────────────────────

    #[test]
    fn placed_inbox_note_hidden_from_active_list() {
        let dir = tempdir().unwrap();
        let inbox = make_inbox_db(&dir);
        let (_ws, ws_path) = make_workspace(&dir, "TestWS");
        let note = make_inbox_note(&inbox, "Note To Place", "Some content");

        assert_eq!(inbox.list_notes().unwrap().len(), 1);

        let ws = WorkspaceDb::open(&ws_path).unwrap();
        let room_id = default_room_id(&ws);
        place_inbox_note(
            &inbox,
            &PlacementRequest {
                inbox_note_id: note.id.clone(),
                workspace_path: ws_path,
                destination: PlaceDestination::LooseInRoom { room_id },
            },
        )
        .unwrap();

        assert!(inbox.list_notes().unwrap().is_empty());
    }

    // ── document_json preserved ───────────────────────────────────────────────

    #[test]
    fn document_json_preserved() {
        let dir = tempdir().unwrap();
        let inbox = make_inbox_db(&dir);
        let (_ws, ws_path) = make_workspace(&dir, "TestWS");
        let doc_json = r#"{"schema_version":1,"blocks":[{"id":"blk001","kind":{"type":"heading","level":1,"text":"Heading"}}]}"#;
        let note = InboxNote {
            id: new_note_id(),
            title: "Structured Note".to_string(),
            body: "# Heading".to_string(),
            document_json: Some(doc_json.to_string()),
            auto_titled: false,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            word_count: 1,
            is_pinned: false,
            is_archived: false,
            placed_at: None,
            placed_workspace_path: None,
            placed_workspace_note_id: None,
            placed_destination_label: None,
        };
        inbox.upsert_note(&note).unwrap();
        let ws = WorkspaceDb::open(&ws_path).unwrap();
        let room_id = default_room_id(&ws);

        let result = place_inbox_note(
            &inbox,
            &PlacementRequest {
                inbox_note_id: note.id.clone(),
                workspace_path: ws_path.clone(),
                destination: PlaceDestination::LooseInRoom { room_id },
            },
        )
        .unwrap();

        let ws2 = WorkspaceDb::open(&ws_path).unwrap();
        let ws_note = ws2.get_note(&result.workspace_note_id).unwrap().unwrap();
        assert!(ws_note.document_json.is_some());
        assert!(ws_note.document_json.unwrap().contains("Heading"));
    }

    // ── title and body preserved ──────────────────────────────────────────────

    #[test]
    fn title_body_preserved() {
        let dir = tempdir().unwrap();
        let inbox = make_inbox_db(&dir);
        let (_ws, ws_path) = make_workspace(&dir, "TestWS");
        let note = make_inbox_note(&inbox, "Important Title", "Important content here.");
        let ws = WorkspaceDb::open(&ws_path).unwrap();
        let room_id = default_room_id(&ws);

        let result = place_inbox_note(
            &inbox,
            &PlacementRequest {
                inbox_note_id: note.id.clone(),
                workspace_path: ws_path.clone(),
                destination: PlaceDestination::LooseInRoom { room_id },
            },
        )
        .unwrap();

        let ws2 = WorkspaceDb::open(&ws_path).unwrap();
        let ws_note = ws2.get_note(&result.workspace_note_id).unwrap().unwrap();
        assert_eq!(ws_note.title, "Important Title");
        assert_eq!(ws_note.body, "Important content here.");
    }

    // ── Pin follows placed note ───────────────────────────────────────────────

    #[test]
    fn pin_follows_placed_note() {
        let dir = tempdir().unwrap();
        let inbox = make_inbox_db(&dir);
        let (_ws, ws_path) = make_workspace(&dir, "TestWS");
        let mut note = make_inbox_note(&inbox, "Pinned Note", "Content");
        note.is_pinned = true;
        inbox.upsert_note(&note).unwrap();
        inbox
            .pin_note("inbox_note", &note.id, "", "Pinned Note", "Content")
            .unwrap();

        let ws = WorkspaceDb::open(&ws_path).unwrap();
        let room_id = default_room_id(&ws);

        let result = place_inbox_note(
            &inbox,
            &PlacementRequest {
                inbox_note_id: note.id.clone(),
                workspace_path: ws_path.clone(),
                destination: PlaceDestination::LooseInRoom { room_id },
            },
        )
        .unwrap();

        let pins = inbox.list_pins().unwrap();
        assert_eq!(pins.len(), 1, "pin should be transferred, not removed");
        assert_eq!(pins[0].target_kind, "workspace_note");
        assert_eq!(pins[0].target_id, result.workspace_note_id);
        assert_eq!(
            pins[0].workspace_path,
            ws_path.to_string_lossy().to_string()
        );
        assert!(!inbox.is_note_pinned("inbox_note", &note.id, ""));
    }

    // ── Recent state updated ──────────────────────────────────────────────────

    #[test]
    fn recent_state_updates_after_placement() {
        let dir = tempdir().unwrap();
        let inbox = make_inbox_db(&dir);
        let (_ws, ws_path) = make_workspace(&dir, "TestWS");
        let note = make_inbox_note(&inbox, "Note", "Content");
        let ws = WorkspaceDb::open(&ws_path).unwrap();
        let room_id = default_room_id(&ws);

        let result = place_inbox_note(
            &inbox,
            &PlacementRequest {
                inbox_note_id: note.id.clone(),
                workspace_path: ws_path.clone(),
                destination: PlaceDestination::LooseInRoom { room_id },
            },
        )
        .unwrap();

        let recents = inbox.list_recent(10).unwrap();
        let placed = recents
            .iter()
            .find(|r| r.target_id == result.workspace_note_id);
        assert!(placed.is_some(), "placed note should appear in recents");
        assert_eq!(placed.unwrap().target_kind, "workspace_note");
    }

    // ── Destination label formatting ──────────────────────────────────────────

    #[test]
    fn destination_label_loose_notes() {
        let dir = tempdir().unwrap();
        let inbox = make_inbox_db(&dir);
        let (_ws, ws_path) = make_workspace(&dir, "MyWorkspace");
        let note = make_inbox_note(&inbox, "Note", "Content");
        let ws = WorkspaceDb::open(&ws_path).unwrap();
        let room_id = default_room_id(&ws);

        let result = place_inbox_note(
            &inbox,
            &PlacementRequest {
                inbox_note_id: note.id,
                workspace_path: ws_path,
                destination: PlaceDestination::LooseInRoom { room_id },
            },
        )
        .unwrap();

        assert!(result.destination_label.contains("MyWorkspace"));
        assert!(result.destination_label.contains("Main Room"));
    }
}
