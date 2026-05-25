pub mod providers;
pub mod query;
pub mod ranking;
pub mod result;
pub mod snippet;

use crate::inbox::InboxDb;
use crate::known_workspaces::KnownWorkspaceRegistry;
use crate::workspace::WorkspaceDb;
use providers::{search_workspace_path, InboxProvider, WorkspaceProvider};
pub use query::SearchQuery;
pub use result::{NoteSourceKind, SearchResult};
use std::path::PathBuf;

// ── Search scope ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum SearchScope {
    /// The currently focused .water workspace only.
    CurrentWorkspace,
    /// The Global Inbox only.
    Inbox,
    /// Current workspace + Inbox.
    CurrentWorkspaceAndInbox,
    /// A specific subset of known workspaces.
    #[allow(dead_code)]
    SelectedWorkspaces(Vec<PathBuf>),
    /// All workspaces in the known registry (no Inbox).
    AllKnownWorkspaces,
    /// All workspaces in the known registry + Inbox.
    AllKnownWorkspacesAndInbox,
}

impl SearchScope {
    pub fn display_label(&self) -> &'static str {
        match self {
            SearchScope::CurrentWorkspace => "Workspace",
            SearchScope::Inbox => "Inbox",
            SearchScope::CurrentWorkspaceAndInbox => "Workspace + Inbox",
            SearchScope::SelectedWorkspaces(_) => "Selected",
            SearchScope::AllKnownWorkspaces => "All Workspaces",
            SearchScope::AllKnownWorkspacesAndInbox => "All + Inbox",
        }
    }

    pub fn all_known(include_inbox: bool) -> Self {
        if include_inbox {
            SearchScope::AllKnownWorkspacesAndInbox
        } else {
            SearchScope::AllKnownWorkspaces
        }
    }
}

// ── Search filters ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct SearchFilters {
    pub pinned_only: bool,
    pub has_checklist: bool,
    pub has_image: bool,
    pub has_links: bool,
}

// ── Search output ─────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct SearchOutput {
    pub results: Vec<SearchResult>,
    /// Workspace display strings for files that couldn't be opened.
    pub unavailable_workspaces: Vec<String>,
}

// ── run_search ────────────────────────────────────────────────────────────────

const MAX_RESULTS: usize = 200;
/// Max results shown when query is empty (recent/pinned context).
const MAX_EMPTY_RESULTS: usize = 30;

pub fn run_search(
    query: &SearchQuery,
    scope: &SearchScope,
    filters: &SearchFilters,
    inbox_db: Option<&InboxDb>,
    workspace_db: Option<&WorkspaceDb>,
    known_workspaces: &KnownWorkspaceRegistry,
) -> SearchOutput {
    let mut output = SearchOutput::default();
    let mut results: Vec<SearchResult> = Vec::new();

    let focused_ws_path: Option<PathBuf> = workspace_db.map(|db| db.path.clone());

    // Collect results from each requested source.
    match scope {
        SearchScope::Inbox => {
            if let Some(inbox) = inbox_db {
                results.extend(InboxProvider { db: inbox }.search(query));
            }
        }

        SearchScope::CurrentWorkspace => {
            if let Some(ws) = workspace_db {
                results.extend(
                    WorkspaceProvider {
                        db: ws,
                        workspace_name: ws.workspace_name(),
                        workspace_path: ws.path.clone(),
                        inbox_db,
                    }
                    .search(query),
                );
            }
        }

        SearchScope::CurrentWorkspaceAndInbox => {
            if let Some(ws) = workspace_db {
                results.extend(
                    WorkspaceProvider {
                        db: ws,
                        workspace_name: ws.workspace_name(),
                        workspace_path: ws.path.clone(),
                        inbox_db,
                    }
                    .search(query),
                );
            }
            if let Some(inbox) = inbox_db {
                results.extend(InboxProvider { db: inbox }.search(query));
            }
        }

        SearchScope::SelectedWorkspaces(paths) => {
            for path in paths {
                search_workspace_path(
                    path,
                    query,
                    inbox_db,
                    &mut results,
                    &mut output.unavailable_workspaces,
                );
            }
        }

        SearchScope::AllKnownWorkspaces => {
            for kw in known_workspaces.list() {
                search_workspace_path(
                    &kw.path,
                    query,
                    inbox_db,
                    &mut results,
                    &mut output.unavailable_workspaces,
                );
            }
        }

        SearchScope::AllKnownWorkspacesAndInbox => {
            for kw in known_workspaces.list() {
                search_workspace_path(
                    &kw.path,
                    query,
                    inbox_db,
                    &mut results,
                    &mut output.unavailable_workspaces,
                );
            }
            if let Some(inbox) = inbox_db {
                results.extend(InboxProvider { db: inbox }.search(query));
            }
        }
    }

    // Apply filters.
    if filters.pinned_only {
        results.retain(|r| r.is_pinned);
    }
    if filters.has_checklist {
        results.retain(|r| r.has_checklist);
    }
    if filters.has_image {
        results.retain(|r| r.has_image);
    }
    if filters.has_links {
        results.retain(|r| r.has_links);
    }

    // Rank: apply boosts then sort.
    ranking::rank(&mut results, query, focused_ws_path.as_deref());

    // Limit result count.
    let cap = if query.is_empty() {
        MAX_EMPTY_RESULTS
    } else {
        MAX_RESULTS
    };
    results.truncate(cap);

    output.results = results;
    output
}

/// Choose a sensible default scope given the current app state.
pub fn default_scope(has_focused_workspace: bool) -> SearchScope {
    if has_focused_workspace {
        SearchScope::CurrentWorkspace
    } else {
        SearchScope::Inbox
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inbox::{InboxDb, InboxNote};
    use crate::known_workspaces::KnownWorkspaceRegistry;
    use tempfile::tempdir;

    fn known_ws_registry() -> KnownWorkspaceRegistry {
        let dir = tempdir().unwrap();
        let path = dir.path().join("kw.json");
        std::mem::forget(dir);
        KnownWorkspaceRegistry::load(&path)
    }

    fn open_inbox(path: &std::path::Path) -> InboxDb {
        InboxDb::open(path).unwrap()
    }

    fn push_note(db: &InboxDb, id: &str, title: &str, body: &str) {
        db.upsert_note(&InboxNote {
            id: id.to_string(),
            title: title.to_string(),
            body: body.to_string(),
            document_json: None,
            auto_titled: false,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-05-22T00:00:00Z".to_string(),
            word_count: 3,
            is_pinned: false,
            is_archived: false,
            placed_at: None,
            placed_workspace_path: None,
            placed_workspace_note_id: None,
            placed_destination_label: None,
        })
        .unwrap();
    }

    #[test]
    fn inbox_scope_only_searches_inbox() {
        let dir = tempdir().unwrap();
        let inbox = open_inbox(&dir.path().join("inbox.db"));
        push_note(&inbox, "n1", "Banana smoothie", "blend it");

        let kw = known_ws_registry();
        let q = SearchQuery::parse("banana");
        let out = run_search(
            &q,
            &SearchScope::Inbox,
            &Default::default(),
            Some(&inbox),
            None,
            &kw,
        );

        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].note_id, "n1");
    }

    #[test]
    fn empty_query_capped_at_small_limit() {
        let dir = tempdir().unwrap();
        let inbox = open_inbox(&dir.path().join("inbox.db"));
        for i in 0..50 {
            push_note(&inbox, &format!("n{i}"), &format!("Note {i}"), "body");
        }

        let kw = known_ws_registry();
        let q = SearchQuery::parse("");
        let out = run_search(
            &q,
            &SearchScope::Inbox,
            &Default::default(),
            Some(&inbox),
            None,
            &kw,
        );

        assert!(out.results.len() <= MAX_EMPTY_RESULTS);
    }

    #[test]
    fn filter_pinned_only() {
        let dir = tempdir().unwrap();
        let inbox = open_inbox(&dir.path().join("inbox.db"));
        push_note(&inbox, "n1", "Pinned note", "content");
        push_note(&inbox, "n2", "Regular note", "content");
        inbox
            .pin_note("inbox_note", "n1", "", "Pinned note", "content")
            .unwrap();
        // Re-upsert n1 with is_pinned=true so list_notes reflects it.
        inbox
            .upsert_note(&InboxNote {
                id: "n1".into(),
                title: "Pinned note".into(),
                body: "content".into(),
                document_json: None,
                auto_titled: false,
                created_at: "2026-01-01T00:00:00Z".into(),
                updated_at: "2026-05-22T00:00:00Z".into(),
                word_count: 1,
                is_pinned: true,
                is_archived: false,
                placed_at: None,
                placed_workspace_path: None,
                placed_workspace_note_id: None,
                placed_destination_label: None,
            })
            .unwrap();

        let kw = known_ws_registry();
        let q = SearchQuery::parse("");
        let filters = SearchFilters {
            pinned_only: true,
            ..Default::default()
        };
        let out = run_search(&q, &SearchScope::Inbox, &filters, Some(&inbox), None, &kw);

        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].note_id, "n1");
        assert!(out.results[0].is_pinned);
    }

    #[test]
    fn default_scope_with_workspace_is_current_workspace() {
        assert_eq!(default_scope(true), SearchScope::CurrentWorkspace);
        assert_eq!(default_scope(false), SearchScope::Inbox);
    }

    #[test]
    fn missing_workspace_recorded_in_output() {
        let dir = tempdir().unwrap();
        let kw_path = dir.path().join("kw.json");
        let mut kw = KnownWorkspaceRegistry::load(&kw_path);
        kw.add_or_update(crate::known_workspaces::KnownWorkspace {
            path: PathBuf::from("/nonexistent/ghost.water"),
            display_name: "Ghost".to_string(),
            last_opened_at: "2026-01-01T00:00:00Z".to_string(),
            last_focused_at: "2026-01-01T00:00:00Z".to_string(),
            last_room_id: None,
            last_note_id: None,
            last_container_kind: None,
            last_container_id: None,
        });

        let q = SearchQuery::parse("x");
        let out = run_search(
            &q,
            &SearchScope::AllKnownWorkspaces,
            &Default::default(),
            None,
            None,
            &kw,
        );

        assert!(out.results.is_empty());
        assert!(!out.unavailable_workspaces.is_empty());
    }
}
