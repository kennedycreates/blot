//! SQLite-backed `.water` workspace for Blot.
//!
//! Blot owns the `blot_` prefixed tables in the workspace SQLite file.
//! The shared `notes` and `note_placements` tables are also defined here
//! since Blot is the primary note editor.
//!
//! Schema version history:
//! - 1 (Prompt 4): initial Blot workspace schema.

use rusqlite::{params, Connection, Result as SqlResult};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: i64 = 1;

// ─── Error ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum WorkspaceError {
    Sql(rusqlite::Error),
    Io(std::io::Error),
    SchemaTooNew {
        file_version: i64,
        app_version: i64,
    },
    NotFound(String),
    #[allow(dead_code)]
    Invalid(String),
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sql(e) => write!(f, "workspace DB error: {e}"),
            Self::Io(e) => write!(f, "workspace I/O error: {e}"),
            Self::SchemaTooNew { file_version, app_version } => write!(
                f,
                "workspace schema v{file_version} is newer than this Blot (supports v{app_version}); \
                 please upgrade Blot to open this workspace"
            ),
            Self::NotFound(msg) => write!(f, "not found: {msg}"),
            Self::Invalid(msg) => write!(f, "invalid workspace: {msg}"),
        }
    }
}

impl Error for WorkspaceError {}

impl From<rusqlite::Error> for WorkspaceError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e)
    }
}

impl From<std::io::Error> for WorkspaceError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

// ─── Data types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Room {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub sort_position: f64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerKind {
    Shelf,
    Pile,
}

impl ContainerKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContainerKind::Shelf => "shelf",
            ContainerKind::Pile => "pile",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "pile" => ContainerKind::Pile,
            _ => ContainerKind::Shelf,
        }
    }

    pub fn display_label(&self) -> &'static str {
        match self {
            ContainerKind::Shelf => "Shelf",
            ContainerKind::Pile => "Pile",
        }
    }
}

/// A Shelf or Pile — both stored in `blot_shelves`, distinguished by `kind`.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Container {
    pub id: String,
    pub room_id: String,
    pub name: String,
    pub kind: ContainerKind,
    pub description: Option<String>,
    pub position: f64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RoomConnection {
    pub id: String,
    /// Always the lexicographically smaller room ID.
    pub room_a_id: String,
    pub room_b_id: String,
    pub connection_type: String,
    pub label: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct WorkspaceNote {
    pub id: String,
    pub title: String,
    pub body: String,
    pub document_json: Option<String>,
    pub auto_titled: bool,
    pub created_at: String,
    pub updated_at: String,
    pub word_count: i64,
    pub is_archived: bool,
}

/// Where a note lives within the workspace.
#[derive(Debug, Clone)]
pub struct NotePlacement {
    pub note_id: String,
    pub room_id: String,
    /// `None` means the note is Loose (not on any shelf or pile).
    pub shelf_id: Option<String>,
    pub position: f64,
}

/// In-memory state for the workspace note currently open in the editor.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct WorkspaceNoteSession {
    pub note_id: Option<String>,
    pub room_id: Option<String>,
    pub shelf_id: Option<String>,
    pub auto_titled: bool,
    pub dirty: bool,
}

impl WorkspaceNoteSession {
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        *self = WorkspaceNoteSession::default();
    }

    #[allow(dead_code)]
    pub fn new_loose_in_room(room_id: &str) -> Self {
        WorkspaceNoteSession {
            note_id: None,
            room_id: Some(room_id.to_string()),
            shelf_id: None,
            auto_titled: true,
            dirty: false,
        }
    }
}

/// A note row joined with placement metadata, used by the search module.
#[derive(Debug, Clone)]
pub struct WorkspaceSearchRow {
    pub note_id: String,
    pub title: String,
    pub body: String,
    pub updated_at: String,
    pub room_name: Option<String>,
    pub shelf_name: Option<String>,
    /// `"shelf"` or `"pile"` — `None` if note is loose.
    pub shelf_kind: Option<String>,
}

// ─── WorkspaceDb ──────────────────────────────────────────────────────────────

pub struct WorkspaceDb {
    conn: Connection,
    pub path: PathBuf,
}

impl std::fmt::Debug for WorkspaceDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceDb")
            .field("path", &self.path)
            .finish()
    }
}

impl WorkspaceDb {
    // ── Constructors ──────────────────────────────────────────────────────

    /// Open an existing `.water` workspace. Returns an error (not a panic) when
    /// the file does not exist or cannot be opened.
    pub fn open(path: &Path) -> Result<Self, WorkspaceError> {
        if !path.exists() {
            return Err(WorkspaceError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("workspace file not found: {}", path.display()),
            )));
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let ws = WorkspaceDb {
            conn,
            path: path.to_path_buf(),
        };
        ws.migrate()?;
        Ok(ws)
    }

    /// Create a brand-new `.water` workspace. Parent directories are created
    /// if they do not exist.
    pub fn create_new(path: &Path, workspace_name: &str) -> Result<Self, WorkspaceError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let ws = WorkspaceDb {
            conn,
            path: path.to_path_buf(),
        };
        ws.apply_schema()?;

        let now = now_iso8601();
        ws.conn.execute(
            "INSERT INTO blot_workspace_meta (id, schema_version, workspace_name, updated_at)
             VALUES (1, ?1, ?2, ?3)",
            params![SCHEMA_VERSION, workspace_name, now],
        )?;

        // Every workspace starts with one default Room.
        ws.create_room("Main Room")?;
        Ok(ws)
    }

    // ── Metadata ──────────────────────────────────────────────────────────

    pub fn workspace_name(&self) -> String {
        self.conn
            .query_row(
                "SELECT workspace_name FROM blot_workspace_meta WHERE id = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap_or_else(|_| {
                self.path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Workspace".to_string())
            })
    }

    pub fn default_room_id(&self) -> Option<String> {
        self.conn
            .query_row(
                "SELECT default_room_id FROM blot_workspace_meta WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .ok()
            .flatten()
    }

    fn set_default_room_id(&self, room_id: &str) -> Result<(), WorkspaceError> {
        self.conn.execute(
            "UPDATE blot_workspace_meta SET default_room_id = ?1 WHERE id = 1",
            params![room_id],
        )?;
        Ok(())
    }

    // ── Schema ────────────────────────────────────────────────────────────

    fn apply_schema(&self) -> Result<(), WorkspaceError> {
        self.conn.execute_batch(SCHEMA_V1)?;
        Ok(())
    }

    fn migrate(&self) -> Result<(), WorkspaceError> {
        let has_meta: bool = self
            .conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='blot_workspace_meta'",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if !has_meta {
            // New file or a non-Blot SQLite: apply Blot's schema additively.
            self.apply_schema()?;
            let name = self
                .path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Workspace".to_string());
            let now = now_iso8601();
            self.conn.execute(
                "INSERT OR IGNORE INTO blot_workspace_meta
                 (id, schema_version, workspace_name, updated_at) VALUES (1, ?1, ?2, ?3)",
                params![SCHEMA_VERSION, name, now],
            )?;
            return Ok(());
        }

        let version: i64 = self
            .conn
            .query_row(
                "SELECT schema_version FROM blot_workspace_meta WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if version > SCHEMA_VERSION {
            return Err(WorkspaceError::SchemaTooNew {
                file_version: version,
                app_version: SCHEMA_VERSION,
            });
        }

        if version < 1 {
            // Back up before migrating existing data.
            let _ = backup_workspace(&self.path);
            self.apply_schema()?;
            self.conn.execute(
                "UPDATE blot_workspace_meta SET schema_version = ?1 WHERE id = 1",
                params![SCHEMA_VERSION],
            )?;
        }

        Ok(())
    }

    // ── Rooms ─────────────────────────────────────────────────────────────

    pub fn create_room(&self, name: &str) -> Result<Room, WorkspaceError> {
        let max_pos: f64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(sort_position), -1.0) FROM blot_rooms",
                [],
                |row| row.get(0),
            )
            .unwrap_or(-1.0);

        let id = new_id();
        let now = now_iso8601();
        self.conn.execute(
            "INSERT INTO blot_rooms (id, name, sort_position, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, name, max_pos + 1.0, now, now],
        )?;

        // If this is the first room, set it as the default.
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM blot_rooms", [], |r| r.get(0))
            .unwrap_or(1);
        if count == 1 {
            let _ = self.set_default_room_id(&id);
        }

        Ok(Room {
            id,
            name: name.to_string(),
            description: None,
            sort_position: max_pos + 1.0,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn list_rooms(&self) -> Result<Vec<Room>, WorkspaceError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, sort_position, created_at, updated_at
             FROM blot_rooms ORDER BY sort_position ASC",
        )?;
        let rooms = stmt
            .query_map([], |row| {
                Ok(Room {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    sort_position: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(rooms)
    }

    pub fn get_room(&self, id: &str) -> Result<Option<Room>, WorkspaceError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, sort_position, created_at, updated_at
             FROM blot_rooms WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(Room {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                sort_position: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        match rows.next() {
            Some(Ok(r)) => Ok(Some(r)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    pub fn rename_room(&self, id: &str, new_name: &str) -> Result<(), WorkspaceError> {
        let now = now_iso8601();
        self.conn.execute(
            "UPDATE blot_rooms SET name = ?1, updated_at = ?2 WHERE id = ?3",
            params![new_name, now, id],
        )?;
        Ok(())
    }

    // ── Room connections (Doors) ───────────────────────────────────────────

    #[allow(dead_code)]
    pub fn create_room_connection(
        &self,
        room_a_id: &str,
        room_b_id: &str,
        connection_type: &str,
    ) -> Result<RoomConnection, WorkspaceError> {
        // Always store room_a < room_b to keep connections undirected.
        let (a, b) = if room_a_id <= room_b_id {
            (room_a_id, room_b_id)
        } else {
            (room_b_id, room_a_id)
        };
        let id = new_id();
        let now = now_iso8601();
        self.conn.execute(
            "INSERT OR IGNORE INTO blot_room_connections
             (id, room_a_id, room_b_id, connection_type, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, a, b, connection_type, now],
        )?;
        Ok(RoomConnection {
            id,
            room_a_id: a.to_string(),
            room_b_id: b.to_string(),
            connection_type: connection_type.to_string(),
            label: None,
            created_at: now,
        })
    }

    #[allow(dead_code)]
    pub fn list_room_connections(&self) -> Result<Vec<RoomConnection>, WorkspaceError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, room_a_id, room_b_id, connection_type, label, created_at
             FROM blot_room_connections",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(RoomConnection {
                    id: row.get(0)?,
                    room_a_id: row.get(1)?,
                    room_b_id: row.get(2)?,
                    connection_type: row.get(3)?,
                    label: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(rows)
    }

    // ── Shelves & Piles ───────────────────────────────────────────────────

    pub fn create_container(
        &self,
        room_id: &str,
        name: &str,
        kind: ContainerKind,
    ) -> Result<Container, WorkspaceError> {
        let max_pos: f64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(position), -1.0) FROM blot_shelves WHERE room_id = ?1",
                params![room_id],
                |row| row.get(0),
            )
            .unwrap_or(-1.0);

        let id = new_id();
        let now = now_iso8601();
        self.conn.execute(
            "INSERT INTO blot_shelves
             (id, room_id, name, kind, position, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, room_id, name, kind.as_str(), max_pos + 1.0, now, now],
        )?;
        Ok(Container {
            id,
            room_id: room_id.to_string(),
            name: name.to_string(),
            kind,
            description: None,
            position: max_pos + 1.0,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn list_containers_in_room(&self, room_id: &str) -> Result<Vec<Container>, WorkspaceError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, room_id, name, kind, description, position, created_at, updated_at
             FROM blot_shelves WHERE room_id = ?1 ORDER BY position ASC",
        )?;
        let rows = stmt
            .query_map(params![room_id], |row| {
                let kind_str: String = row.get(3)?;
                Ok(Container {
                    id: row.get(0)?,
                    room_id: row.get(1)?,
                    name: row.get(2)?,
                    kind: ContainerKind::from_str(&kind_str),
                    description: row.get(4)?,
                    position: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(rows)
    }

    /// Convert a Pile to a Shelf. Notes already in the pile stay in place.
    pub fn convert_pile_to_shelf(&self, pile_id: &str) -> Result<(), WorkspaceError> {
        let now = now_iso8601();
        let rows_changed = self.conn.execute(
            "UPDATE blot_shelves SET kind = 'shelf', updated_at = ?1
             WHERE id = ?2 AND kind = 'pile'",
            params![now, pile_id],
        )?;
        if rows_changed == 0 {
            return Err(WorkspaceError::NotFound(format!(
                "pile with id '{pile_id}' does not exist"
            )));
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn rename_container(&self, id: &str, new_name: &str) -> Result<(), WorkspaceError> {
        let now = now_iso8601();
        self.conn.execute(
            "UPDATE blot_shelves SET name = ?1, updated_at = ?2 WHERE id = ?3",
            params![new_name, now, id],
        )?;
        Ok(())
    }

    // ── Notes ─────────────────────────────────────────────────────────────

    pub fn upsert_note(&self, note: &WorkspaceNote) -> Result<(), WorkspaceError> {
        self.conn.execute(
            "INSERT INTO notes
               (id, title, body, document_json, auto_titled, created_at,
                updated_at, word_count, is_archived)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
               title         = excluded.title,
               body          = excluded.body,
               document_json = excluded.document_json,
               auto_titled   = excluded.auto_titled,
               updated_at    = excluded.updated_at,
               word_count    = excluded.word_count,
               is_archived   = excluded.is_archived",
            params![
                note.id,
                note.title,
                note.body,
                note.document_json,
                note.auto_titled as i64,
                note.created_at,
                note.updated_at,
                note.word_count,
                note.is_archived as i64,
            ],
        )?;
        Ok(())
    }

    pub fn get_note(&self, id: &str) -> Result<Option<WorkspaceNote>, WorkspaceError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, body, document_json, auto_titled, created_at,
                    updated_at, word_count, is_archived
             FROM notes WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(WorkspaceNote {
                id: row.get(0)?,
                title: row.get(1)?,
                body: row.get(2)?,
                document_json: row.get(3)?,
                auto_titled: row.get::<_, i64>(4)? != 0,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
                word_count: row.get(7)?,
                is_archived: row.get::<_, i64>(8)? != 0,
            })
        })?;
        match rows.next() {
            Some(Ok(n)) => Ok(Some(n)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    #[allow(dead_code)]
    pub fn list_all_notes(&self) -> Result<Vec<WorkspaceNote>, WorkspaceError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, body, document_json, auto_titled, created_at,
                    updated_at, word_count, is_archived
             FROM notes WHERE is_archived = 0
             ORDER BY updated_at DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(WorkspaceNote {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    body: row.get(2)?,
                    document_json: row.get(3)?,
                    auto_titled: row.get::<_, i64>(4)? != 0,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                    word_count: row.get(7)?,
                    is_archived: row.get::<_, i64>(8)? != 0,
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(rows)
    }

    // ── Note placements ───────────────────────────────────────────────────

    pub fn set_note_placement(&self, placement: &NotePlacement) -> Result<(), WorkspaceError> {
        let now = now_iso8601();
        let placement_id = new_id();
        self.conn.execute(
            "INSERT INTO note_placements (id, note_id, room_id, shelf_id, position, placed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(note_id) DO UPDATE SET
               room_id   = excluded.room_id,
               shelf_id  = excluded.shelf_id,
               position  = excluded.position,
               placed_at = excluded.placed_at",
            params![
                placement_id,
                placement.note_id,
                placement.room_id,
                placement.shelf_id,
                placement.position,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn get_note_placement(
        &self,
        note_id: &str,
    ) -> Result<Option<NotePlacement>, WorkspaceError> {
        let mut stmt = self.conn.prepare(
            "SELECT note_id, room_id, shelf_id, position
             FROM note_placements WHERE note_id = ?1",
        )?;
        let mut rows = stmt.query_map(params![note_id], |row| {
            Ok(NotePlacement {
                note_id: row.get(0)?,
                room_id: row.get(1)?,
                shelf_id: row.get(2)?,
                position: row.get(3)?,
            })
        })?;
        match rows.next() {
            Some(Ok(p)) => Ok(Some(p)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    /// Create a new note and place it as Loose in the given room.
    /// The returned note has an empty title and body — content is added by the editor.
    #[allow(dead_code)]
    pub fn create_loose_note(&self, room_id: &str) -> Result<WorkspaceNote, WorkspaceError> {
        let id = new_id();
        let now = now_iso8601();
        let note = WorkspaceNote {
            id: id.clone(),
            title: String::new(),
            body: String::new(),
            document_json: None,
            auto_titled: true,
            created_at: now.clone(),
            updated_at: now,
            word_count: 0,
            is_archived: false,
        };
        self.upsert_note(&note)?;
        self.set_note_placement(&NotePlacement {
            note_id: id,
            room_id: room_id.to_string(),
            shelf_id: None,
            position: 0.0,
        })?;
        Ok(note)
    }

    pub fn list_loose_notes(&self, room_id: &str) -> Result<Vec<WorkspaceNote>, WorkspaceError> {
        let mut stmt = self.conn.prepare(
            "SELECT n.id, n.title, n.body, n.document_json, n.auto_titled,
                    n.created_at, n.updated_at, n.word_count, n.is_archived
             FROM notes n
             JOIN note_placements p ON n.id = p.note_id
             WHERE p.room_id = ?1 AND p.shelf_id IS NULL AND n.is_archived = 0
             ORDER BY n.updated_at DESC",
        )?;
        let rows = stmt
            .query_map(params![room_id], |row| {
                Ok(WorkspaceNote {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    body: row.get(2)?,
                    document_json: row.get(3)?,
                    auto_titled: row.get::<_, i64>(4)? != 0,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                    word_count: row.get(7)?,
                    is_archived: row.get::<_, i64>(8)? != 0,
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn list_notes_in_container(
        &self,
        shelf_id: &str,
    ) -> Result<Vec<WorkspaceNote>, WorkspaceError> {
        let mut stmt = self.conn.prepare(
            "SELECT n.id, n.title, n.body, n.document_json, n.auto_titled,
                    n.created_at, n.updated_at, n.word_count, n.is_archived
             FROM notes n
             JOIN note_placements p ON n.id = p.note_id
             WHERE p.shelf_id = ?1 AND n.is_archived = 0
             ORDER BY p.position ASC, n.updated_at DESC",
        )?;
        let rows = stmt
            .query_map(params![shelf_id], |row| {
                Ok(WorkspaceNote {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    body: row.get(2)?,
                    document_json: row.get(3)?,
                    auto_titled: row.get::<_, i64>(4)? != 0,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                    word_count: row.get(7)?,
                    is_archived: row.get::<_, i64>(8)? != 0,
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(rows)
    }

    #[allow(dead_code)]
    pub fn move_note_to_container(
        &self,
        note_id: &str,
        shelf_id: &str,
        room_id: &str,
    ) -> Result<(), WorkspaceError> {
        self.set_note_placement(&NotePlacement {
            note_id: note_id.to_string(),
            room_id: room_id.to_string(),
            shelf_id: Some(shelf_id.to_string()),
            position: 0.0,
        })
    }

    pub fn move_note_to_loose(&self, note_id: &str, room_id: &str) -> Result<(), WorkspaceError> {
        self.set_note_placement(&NotePlacement {
            note_id: note_id.to_string(),
            room_id: room_id.to_string(),
            shelf_id: None,
            position: 0.0,
        })
    }

    pub fn loose_note_count(&self, room_id: &str) -> i64 {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM notes n
                 JOIN note_placements p ON n.id = p.note_id
                 WHERE p.room_id = ?1 AND p.shelf_id IS NULL AND n.is_archived = 0",
                params![room_id],
                |r| r.get(0),
            )
            .unwrap_or(0)
    }

    pub fn container_note_count(&self, shelf_id: &str) -> i64 {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM notes n
                 JOIN note_placements p ON n.id = p.note_id
                 WHERE p.shelf_id = ?1 AND n.is_archived = 0",
                params![shelf_id],
                |r| r.get(0),
            )
            .unwrap_or(0)
    }

    // ── Search ────────────────────────────────────────────────────────────

    /// Return all non-archived notes with their placement metadata joined in.
    /// Used by the search providers; filtering and ranking happen in Rust.
    pub fn search_notes_with_placement(&self) -> Result<Vec<WorkspaceSearchRow>, WorkspaceError> {
        let mut stmt = self.conn.prepare(
            "SELECT
                 n.id, n.title, n.body, n.updated_at,
                 r.name  AS room_name,
                 s.name  AS shelf_name,
                 s.kind  AS shelf_kind
             FROM notes n
             LEFT JOIN note_placements p ON n.id = p.note_id
             LEFT JOIN blot_rooms       r ON p.room_id  = r.id
             LEFT JOIN blot_shelves     s ON p.shelf_id = s.id
             WHERE n.is_archived = 0
             ORDER BY n.updated_at DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(WorkspaceSearchRow {
                    note_id: row.get(0)?,
                    title: row.get(1)?,
                    body: row.get(2)?,
                    updated_at: row.get(3)?,
                    room_name: row.get(4)?,
                    shelf_name: row.get(5)?,
                    shelf_kind: row.get(6)?,
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(rows)
    }
}

// ─── Utilities ────────────────────────────────────────────────────────────────

pub(crate) fn new_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    format!("{t:016x}{n:016x}")
}

pub(crate) fn now_iso8601() -> String {
    // Try GLib (available in the GTK app process; falls back gracefully).
    glib::DateTime::now_local()
        .and_then(|dt| dt.format_iso8601())
        .map(|s| s.to_string())
        .unwrap_or_else(|_| {
            // Fallback for non-GTK contexts (tests without a display).
            let secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let y = 1970 + secs / 31_557_600;
            format!("{y:04}-01-01T00:00:00Z")
        })
}

fn backup_workspace(path: &Path) -> std::io::Result<()> {
    let backup = path.with_extension("water.bak");
    std::fs::copy(path, backup)?;
    Ok(())
}

// ─── Schema SQL ───────────────────────────────────────────────────────────────

const SCHEMA_V1: &str = "
-- Single-row workspace metadata table.
CREATE TABLE IF NOT EXISTS blot_workspace_meta (
    id             INTEGER PRIMARY KEY DEFAULT 1,
    schema_version INTEGER NOT NULL DEFAULT 1,
    workspace_name TEXT    NOT NULL DEFAULT 'Workspace',
    default_room_id     TEXT,
    last_open_note_id   TEXT,
    updated_at     TEXT NOT NULL
);

-- Top-level organizational units inside a workspace.
CREATE TABLE IF NOT EXISTS blot_rooms (
    id            TEXT NOT NULL PRIMARY KEY,
    name          TEXT NOT NULL,
    description   TEXT,
    atmosphere_json TEXT,
    map_x         REAL NOT NULL DEFAULT 0.0,
    map_y         REAL NOT NULL DEFAULT 0.0,
    map_width     REAL NOT NULL DEFAULT 200.0,
    map_height    REAL NOT NULL DEFAULT 120.0,
    sort_position REAL NOT NULL DEFAULT 0.0,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

-- Doors between rooms (undirected).
-- room_a_id < room_b_id is enforced in application code.
CREATE TABLE IF NOT EXISTS blot_room_connections (
    id              TEXT NOT NULL PRIMARY KEY,
    room_a_id       TEXT NOT NULL REFERENCES blot_rooms(id),
    room_b_id       TEXT NOT NULL REFERENCES blot_rooms(id),
    connection_type TEXT NOT NULL DEFAULT 'normal',
    label           TEXT,
    created_at      TEXT NOT NULL,
    UNIQUE(room_a_id, room_b_id)
);

-- Shelves (intentional) and Piles (loose/transitional), unified by the kind column.
-- Converting a Pile to a Shelf: UPDATE blot_shelves SET kind = 'shelf' WHERE id = ?
CREATE TABLE IF NOT EXISTS blot_shelves (
    id          TEXT NOT NULL PRIMARY KEY,
    room_id     TEXT NOT NULL REFERENCES blot_rooms(id),
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL DEFAULT 'shelf',
    description TEXT,
    position    REAL NOT NULL DEFAULT 0.0,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

-- Workspace notes. Shared base schema + Blot additions.
-- Other Watercolor apps may read: id, title, body, created_at, updated_at.
-- Blot-specific: document_json, auto_titled, word_count, is_archived.
CREATE TABLE IF NOT EXISTS notes (
    id                    TEXT NOT NULL PRIMARY KEY,
    title                 TEXT NOT NULL DEFAULT '',
    body                  TEXT NOT NULL DEFAULT '',
    document_json         TEXT,
    auto_titled           INTEGER NOT NULL DEFAULT 1,
    created_at            TEXT NOT NULL,
    updated_at            TEXT NOT NULL,
    word_count            INTEGER NOT NULL DEFAULT 0,
    is_archived           INTEGER NOT NULL DEFAULT 0,
    redirects_to_note_id  TEXT
);

-- Where each note lives: room, shelf/pile, or loose (shelf_id IS NULL).
-- Invariant: each note has exactly one placement row.
CREATE TABLE IF NOT EXISTS note_placements (
    id        TEXT NOT NULL PRIMARY KEY,
    note_id   TEXT NOT NULL UNIQUE REFERENCES notes(id),
    room_id   TEXT NOT NULL REFERENCES blot_rooms(id),
    shelf_id  TEXT         REFERENCES blot_shelves(id),
    position  REAL NOT NULL DEFAULT 0.0,
    placed_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_note_placements_room  ON note_placements(room_id);
CREATE INDEX IF NOT EXISTS idx_note_placements_shelf ON note_placements(shelf_id);
CREATE INDEX IF NOT EXISTS idx_notes_updated         ON notes(updated_at DESC);
";

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn open_temp_ws() -> (WorkspaceDb, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.water");
        let ws = WorkspaceDb::create_new(&path, "Test Workspace").unwrap();
        (ws, dir)
    }

    #[test]
    fn create_new_initializes_schema() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("new.water");
        let ws = WorkspaceDb::create_new(&path, "My Workspace").unwrap();
        assert!(path.exists());
        assert_eq!(ws.workspace_name(), "My Workspace");
    }

    #[test]
    fn creates_default_room_on_new() {
        let (ws, _dir) = open_temp_ws();
        let rooms = ws.list_rooms().unwrap();
        assert_eq!(rooms.len(), 1);
        assert_eq!(rooms[0].name, "Main Room");
    }

    #[test]
    fn default_room_id_is_set() {
        let (ws, _dir) = open_temp_ws();
        assert!(ws.default_room_id().is_some());
    }

    #[test]
    fn open_missing_path_returns_error_not_panic() {
        let result = WorkspaceDb::open(std::path::Path::new("/tmp/nonexistent_blot.water"));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("not found") || msg.contains("No such"));
    }

    #[test]
    fn create_room() {
        let (ws, _dir) = open_temp_ws();
        let room = ws.create_room("Research").unwrap();
        assert_eq!(room.name, "Research");
        let rooms = ws.list_rooms().unwrap();
        assert_eq!(rooms.len(), 2);
    }

    #[test]
    fn rename_room() {
        let (ws, _dir) = open_temp_ws();
        let rooms = ws.list_rooms().unwrap();
        ws.rename_room(&rooms[0].id, "Renamed Room").unwrap();
        let rooms2 = ws.list_rooms().unwrap();
        assert_eq!(rooms2[0].name, "Renamed Room");
    }

    #[test]
    fn create_shelf() {
        let (ws, _dir) = open_temp_ws();
        let room = ws.list_rooms().unwrap().into_iter().next().unwrap();
        let shelf = ws
            .create_container(&room.id, "Reference", ContainerKind::Shelf)
            .unwrap();
        assert_eq!(shelf.kind, ContainerKind::Shelf);
        assert_eq!(shelf.name, "Reference");
    }

    #[test]
    fn create_pile() {
        let (ws, _dir) = open_temp_ws();
        let room = ws.list_rooms().unwrap().into_iter().next().unwrap();
        let pile = ws
            .create_container(&room.id, "Unsorted", ContainerKind::Pile)
            .unwrap();
        assert_eq!(pile.kind, ContainerKind::Pile);
    }

    #[test]
    fn convert_pile_to_shelf() {
        let (ws, _dir) = open_temp_ws();
        let room = ws.list_rooms().unwrap().into_iter().next().unwrap();
        let pile = ws
            .create_container(&room.id, "Temp", ContainerKind::Pile)
            .unwrap();
        ws.convert_pile_to_shelf(&pile.id).unwrap();
        let containers = ws.list_containers_in_room(&room.id).unwrap();
        let found = containers.iter().find(|c| c.id == pile.id).unwrap();
        assert_eq!(found.kind, ContainerKind::Shelf);
    }

    #[test]
    fn convert_nonexistent_pile_returns_error() {
        let (ws, _dir) = open_temp_ws();
        let result = ws.convert_pile_to_shelf("bad-id");
        assert!(result.is_err());
    }

    #[test]
    fn create_loose_note_and_list() {
        let (ws, _dir) = open_temp_ws();
        let room = ws.list_rooms().unwrap().into_iter().next().unwrap();
        ws.create_loose_note(&room.id).unwrap();
        let notes = ws.list_loose_notes(&room.id).unwrap();
        // Blank notes created via create_loose_note are saved; listing includes them.
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn move_note_to_shelf_and_back() {
        let (ws, _dir) = open_temp_ws();
        let room = ws.list_rooms().unwrap().into_iter().next().unwrap();
        let note = ws.create_loose_note(&room.id).unwrap();
        let shelf = ws
            .create_container(&room.id, "Archive", ContainerKind::Shelf)
            .unwrap();

        ws.move_note_to_container(&note.id, &shelf.id, &room.id)
            .unwrap();
        assert_eq!(ws.loose_note_count(&room.id), 0);
        assert_eq!(ws.container_note_count(&shelf.id), 1);

        ws.move_note_to_loose(&note.id, &room.id).unwrap();
        assert_eq!(ws.loose_note_count(&room.id), 1);
        assert_eq!(ws.container_note_count(&shelf.id), 0);
    }

    #[test]
    fn upsert_and_load_document_json() {
        let (ws, _dir) = open_temp_ws();
        let room = ws.list_rooms().unwrap().into_iter().next().unwrap();
        let mut note = ws.create_loose_note(&room.id).unwrap();
        note.title = "My note".to_string();
        note.body = "Hello world".to_string();
        note.document_json = Some(r#"{"schema_version":1,"blocks":[]}"#.to_string());
        ws.upsert_note(&note).unwrap();

        let loaded = ws.get_note(&note.id).unwrap().unwrap();
        assert_eq!(loaded.title, "My note");
        assert!(loaded.document_json.is_some());
    }

    #[test]
    fn room_connection_stored_and_listed() {
        let (ws, _dir) = open_temp_ws();
        let r1 = ws.create_room("Room A").unwrap();
        let r2 = ws.create_room("Room B").unwrap();
        ws.create_room_connection(&r1.id, &r2.id, "normal").unwrap();
        let conns = ws.list_room_connections().unwrap();
        assert_eq!(conns.len(), 1);
        assert_eq!(conns[0].connection_type, "normal");
    }

    #[test]
    fn open_existing_workspace() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("persist.water");
        {
            let ws = WorkspaceDb::create_new(&path, "Persist Test").unwrap();
            ws.create_room("Second Room").unwrap();
        }
        let ws2 = WorkspaceDb::open(&path).unwrap();
        let rooms = ws2.list_rooms().unwrap();
        assert_eq!(rooms.len(), 2);
        assert_eq!(ws2.workspace_name(), "Persist Test");
    }
}
