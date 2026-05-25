use std::path::PathBuf;

/// Where a note lives in the Blot hierarchy.
#[derive(Debug, Clone, PartialEq)]
pub enum NoteLocation {
    Inbox,
    WorkspaceLoose {
        room_name: String,
    },
    WorkspaceShelf {
        room_name: String,
        container_name: String,
    },
    WorkspacePile {
        room_name: String,
        container_name: String,
    },
}

impl NoteLocation {
    pub fn short_label(&self) -> String {
        match self {
            NoteLocation::Inbox => "Inbox".to_string(),
            NoteLocation::WorkspaceLoose { room_name } => {
                format!("{room_name} › Loose Notes")
            }
            NoteLocation::WorkspaceShelf {
                room_name,
                container_name,
            } => {
                format!("{room_name} › {container_name}")
            }
            NoteLocation::WorkspacePile {
                room_name,
                container_name,
            } => {
                format!("{room_name} › {container_name} (Pile)")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteSourceKind {
    InboxNote,
    WorkspaceNote,
}

/// A single search result with all metadata needed for the result card.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub note_id: String,
    pub title: String,
    /// Short excerpt near the first match.
    pub snippet: String,
    /// ISO 8601 / display date string.
    pub updated_at: String,
    pub location: NoteLocation,
    /// `None` for Inbox notes.
    pub workspace_name: Option<String>,
    pub workspace_path: Option<PathBuf>,
    pub is_pinned: bool,
    pub source_kind: NoteSourceKind,
    /// Heuristic flags from body content.
    pub has_checklist: bool,
    pub has_image: bool,
    pub has_links: bool,
    /// Internal ranking score — higher = shown first. Mutable so ranking can update it.
    pub score: f32,
}

impl SearchResult {
    /// Full breadcrumb label including workspace name when present.
    pub fn full_location_label(&self) -> String {
        match &self.location {
            NoteLocation::Inbox => "Inbox".to_string(),
            other => match &self.workspace_name {
                Some(ws) => format!("{ws} › {}", other.short_label()),
                None => other.short_label(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result_in(location: NoteLocation, ws_name: Option<&str>) -> SearchResult {
        SearchResult {
            note_id: "n1".into(),
            title: "Test".into(),
            snippet: "snippet".into(),
            updated_at: "2026-05-22T00:00:00Z".into(),
            location,
            workspace_name: ws_name.map(|s| s.to_string()),
            workspace_path: ws_name.map(|_| PathBuf::from("/notes.water")),
            is_pinned: false,
            source_kind: NoteSourceKind::InboxNote,
            has_checklist: false,
            has_image: false,
            has_links: false,
            score: 0.0,
        }
    }

    #[test]
    fn inbox_label() {
        let r = result_in(NoteLocation::Inbox, None);
        assert_eq!(r.full_location_label(), "Inbox");
    }

    #[test]
    fn workspace_loose_label_with_workspace_name() {
        let r = result_in(
            NoteLocation::WorkspaceLoose {
                room_name: "Research".into(),
            },
            Some("My Notes"),
        );
        assert_eq!(r.full_location_label(), "My Notes › Research › Loose Notes");
    }

    #[test]
    fn workspace_shelf_label() {
        let r = result_in(
            NoteLocation::WorkspaceShelf {
                room_name: "Writing".into(),
                container_name: "Essays".into(),
            },
            Some("Projects"),
        );
        assert_eq!(r.full_location_label(), "Projects › Writing › Essays");
    }

    #[test]
    fn workspace_pile_label() {
        let r = result_in(
            NoteLocation::WorkspacePile {
                room_name: "Ideas".into(),
                container_name: "Drafts".into(),
            },
            Some("Brain"),
        );
        assert_eq!(r.full_location_label(), "Brain › Ideas › Drafts (Pile)");
    }
}
