/// A snapshot of a note at a point in time.
/// Stored in `inbox_note_versions` (inbox.rs) or `note_versions` (workspace.rs).
/// Defined here to break the circular-import cycle between inbox, workspace, and ops.
#[derive(Debug, Clone)]
pub struct NoteVersion {
    pub id: String,
    pub note_id: String,
    pub title: String,
    pub body: String,
    pub document_json: Option<String>,
    pub created_at: String,
    pub reason: String,
    pub is_bookmark: bool,
    pub bookmark_name: Option<String>,
    /// `"auto"` | `"manual"` — how the bookmark was created.
    pub bookmark_kind: Option<String>,
    /// Correlates all versions created by one composite operation (e.g. merge).
    pub operation_id: Option<String>,
}
