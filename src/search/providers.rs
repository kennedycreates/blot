use crate::inbox::InboxDb;
use crate::search::query::SearchQuery;
use crate::search::result::{NoteLocation, NoteSourceKind, SearchResult};
use crate::search::snippet::extract_snippet;
use crate::workspace::WorkspaceDb;
use std::path::{Path, PathBuf};

// ── Inbox provider ────────────────────────────────────────────────────────────

/// Searches the Global Inbox.
pub struct InboxProvider<'a> {
    pub db: &'a InboxDb,
}

impl<'a> InboxProvider<'a> {
    pub fn search(&self, query: &SearchQuery) -> Vec<SearchResult> {
        let notes = match self.db.list_notes() {
            Ok(n) => n,
            Err(e) => {
                eprintln!("blot search: inbox read error: {e}");
                return Vec::new();
            }
        };

        notes
            .into_iter()
            .filter(|n| query.matches_note(&n.title, &n.body))
            .map(|n| {
                let base_score = if query.is_empty() {
                    0.0
                } else {
                    query.score_title(&n.title) + query.score_body(&n.body)
                };
                let has_checklist = n.body.contains("- [ ]")
                    || n.body.contains("- [x]")
                    || n.body.contains("- [X]");
                let has_image = n.body.contains("![");
                let has_links = n.body.contains("[[") || n.body.contains("](");
                SearchResult {
                    note_id: n.id,
                    title: if n.title.is_empty() {
                        "(Untitled)".to_string()
                    } else {
                        n.title
                    },
                    snippet: extract_snippet(&n.body, &query.terms),
                    updated_at: n.updated_at,
                    location: NoteLocation::Inbox,
                    workspace_name: None,
                    workspace_path: None,
                    is_pinned: n.is_pinned,
                    source_kind: NoteSourceKind::InboxNote,
                    has_checklist,
                    has_image,
                    has_links,
                    score: base_score,
                }
            })
            .collect()
    }
}

// ── Workspace provider ────────────────────────────────────────────────────────

/// Searches one `.water` workspace.
pub struct WorkspaceProvider<'a> {
    pub db: &'a WorkspaceDb,
    pub workspace_name: String,
    pub workspace_path: PathBuf,
    /// Optional inbox DB for checking global pin state of workspace notes.
    pub inbox_db: Option<&'a InboxDb>,
}

impl<'a> WorkspaceProvider<'a> {
    pub fn search(&self, query: &SearchQuery) -> Vec<SearchResult> {
        let rows = match self.db.search_notes_with_placement() {
            Ok(r) => r,
            Err(e) => {
                eprintln!(
                    "blot search: workspace read error [{}]: {e}",
                    self.workspace_name
                );
                return Vec::new();
            }
        };

        let ws_path_str = self.workspace_path.to_string_lossy().to_string();

        rows.into_iter()
            .filter(|row| query.matches_note(&row.title, &row.body))
            .map(|row| {
                let base_score = if query.is_empty() {
                    0.0
                } else {
                    query.score_title(&row.title) + query.score_body(&row.body)
                };

                let location = placement_to_location(
                    row.room_name.as_deref(),
                    row.shelf_name.as_deref(),
                    row.shelf_kind.as_deref(),
                );

                let is_pinned = self
                    .inbox_db
                    .map(|db| db.is_note_pinned("workspace_note", &row.note_id, &ws_path_str))
                    .unwrap_or(false);

                let has_checklist = row.body.contains("- [ ]")
                    || row.body.contains("- [x]")
                    || row.body.contains("- [X]");
                let has_image = row.body.contains("![");
                let has_links = row.body.contains("[[") || row.body.contains("](");

                SearchResult {
                    note_id: row.note_id,
                    title: if row.title.is_empty() {
                        "(Untitled)".to_string()
                    } else {
                        row.title
                    },
                    snippet: extract_snippet(&row.body, &query.terms),
                    updated_at: row.updated_at,
                    location,
                    workspace_name: Some(self.workspace_name.clone()),
                    workspace_path: Some(self.workspace_path.clone()),
                    is_pinned,
                    source_kind: NoteSourceKind::WorkspaceNote,
                    has_checklist,
                    has_image,
                    has_links,
                    score: base_score,
                }
            })
            .collect()
    }
}

fn placement_to_location(
    room_name: Option<&str>,
    shelf_name: Option<&str>,
    shelf_kind: Option<&str>,
) -> NoteLocation {
    match (room_name, shelf_name, shelf_kind) {
        (Some(r), Some(s), Some("pile")) => NoteLocation::WorkspacePile {
            room_name: r.to_string(),
            container_name: s.to_string(),
        },
        (Some(r), Some(s), _) => NoteLocation::WorkspaceShelf {
            room_name: r.to_string(),
            container_name: s.to_string(),
        },
        (Some(r), None, _) => NoteLocation::WorkspaceLoose {
            room_name: r.to_string(),
        },
        _ => NoteLocation::WorkspaceLoose {
            room_name: "Workspace".to_string(),
        },
    }
}

/// Open a workspace at `path` and search it. Appends unavailable errors to `errors`.
pub fn search_workspace_path(
    path: &Path,
    query: &SearchQuery,
    inbox_db: Option<&InboxDb>,
    results: &mut Vec<SearchResult>,
    errors: &mut Vec<String>,
) {
    if !path.exists() {
        errors.push(format!("{} (file not found)", path.display()));
        return;
    }
    match WorkspaceDb::open(path) {
        Ok(db) => {
            let provider = WorkspaceProvider {
                workspace_name: db.workspace_name(),
                workspace_path: path.to_path_buf(),
                db: &db,
                inbox_db,
            };
            results.extend(provider.search(query));
        }
        Err(e) => {
            errors.push(format!("{}: {e}", path.display()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inbox::{InboxDb, InboxNote};
    use crate::workspace::WorkspaceDb;
    use tempfile::tempdir;

    fn open_inbox(dir: &tempfile::TempDir) -> InboxDb {
        let path = dir.path().join("inbox.db");
        InboxDb::open(&path).unwrap()
    }

    fn sample_inbox_note(id: &str, title: &str, body: &str) -> InboxNote {
        InboxNote {
            id: id.to_string(),
            title: title.to_string(),
            body: body.to_string(),
            document_json: None,
            auto_titled: false,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-05-01T00:00:00Z".to_string(),
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
    fn inbox_provider_returns_matching_note() {
        let dir = tempdir().unwrap();
        std::mem::forget(dir.path().to_path_buf()); // keep path alive
        let db = open_inbox(&dir);
        db.upsert_note(&sample_inbox_note(
            "n1",
            "Rocket Science",
            "exploring space",
        ))
        .unwrap();
        db.upsert_note(&sample_inbox_note("n2", "Cooking tips", "salt and pepper"))
            .unwrap();

        let q = SearchQuery::parse("rocket");
        let provider = InboxProvider { db: &db };
        let results = provider.search(&q);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].note_id, "n1");
        assert_eq!(results[0].source_kind, NoteSourceKind::InboxNote);
        assert!(matches!(results[0].location, NoteLocation::Inbox));
    }

    #[test]
    fn inbox_provider_empty_query_returns_all() {
        let dir = tempdir().unwrap();
        let db = open_inbox(&dir);
        db.upsert_note(&sample_inbox_note("n1", "A", "body"))
            .unwrap();
        db.upsert_note(&sample_inbox_note("n2", "B", "body"))
            .unwrap();

        let q = SearchQuery::parse("");
        let provider = InboxProvider { db: &db };
        let results = provider.search(&q);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn workspace_provider_returns_matching_note() {
        let dir = tempdir().unwrap();
        let ws_path = dir.path().join("test.water");
        let ws = WorkspaceDb::create_new(&ws_path, "Test WS").unwrap();

        let room = ws.list_rooms().unwrap().into_iter().next().unwrap();
        let note = ws.create_loose_note(&room.id).unwrap();
        ws.upsert_note(&crate::workspace::WorkspaceNote {
            id: note.id.clone(),
            title: "Space mission".to_string(),
            body: "launch rocket into orbit".to_string(),
            document_json: None,
            auto_titled: false,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-05-01T00:00:00Z".to_string(),
            word_count: 4,
            is_archived: false,
        })
        .unwrap();

        let q = SearchQuery::parse("rocket");
        let provider = WorkspaceProvider {
            db: &ws,
            workspace_name: "Test WS".to_string(),
            workspace_path: ws_path,
            inbox_db: None,
        };
        let results = provider.search(&q);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source_kind, NoteSourceKind::WorkspaceNote);
    }

    #[test]
    fn missing_workspace_adds_error() {
        let q = SearchQuery::parse("x");
        let mut results = Vec::new();
        let mut errors = Vec::new();
        search_workspace_path(
            Path::new("/nonexistent/ghost.water"),
            &q,
            None,
            &mut results,
            &mut errors,
        );
        assert!(results.is_empty());
        assert!(!errors.is_empty());
        assert!(errors[0].contains("not found"));
    }
}
