//! Registry of known `.water` workspace files.
//!
//! Stored as JSON at `~/.local/share/blot/known_workspaces.json`.
//! Tracks recently opened workspaces so Blot can list them in Desk Mode
//! without requiring Terroir.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownWorkspace {
    /// Absolute path to the `.water` file.
    pub path: PathBuf,
    /// Human-readable name (from workspace metadata or file stem).
    pub display_name: String,
    /// ISO 8601 timestamp of when Blot last opened this workspace.
    pub last_opened_at: String,
    /// ISO 8601 timestamp of when the workspace was last in focus.
    pub last_focused_at: String,
    /// Last room ID the user had open in this workspace.
    #[serde(default)]
    pub last_room_id: Option<String>,
    /// Last note ID open in this workspace.
    #[serde(default)]
    pub last_note_id: Option<String>,
    /// Kind of the last container used for Place Note ("shelf" or "pile").
    /// None means Loose Notes was last used.
    #[serde(default)]
    pub last_container_kind: Option<String>,
    /// ID of the last shelf or pile used for Place Note in this workspace.
    #[serde(default)]
    pub last_container_id: Option<String>,
}

/// In-memory registry of known workspaces. Persists to JSON on every update.
pub struct KnownWorkspaceRegistry {
    pub workspaces: Vec<KnownWorkspace>,
    path: PathBuf,
}

impl KnownWorkspaceRegistry {
    /// Load the registry from `path`. Returns an empty registry if the file
    /// does not exist or is unreadable — never panics.
    pub fn load(path: &Path) -> Self {
        let workspaces = if path.exists() {
            std::fs::read_to_string(path)
                .ok()
                .and_then(|s| serde_json::from_str::<Vec<KnownWorkspace>>(&s).ok())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        KnownWorkspaceRegistry {
            workspaces,
            path: path.to_path_buf(),
        }
    }

    /// Persist the registry to disk. Logs errors; never panics.
    pub fn save(&self) {
        let Ok(json) = serde_json::to_string_pretty(&self.workspaces) else {
            eprintln!("blot: failed to serialize known_workspaces");
            return;
        };
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&self.path, json) {
            eprintln!("blot: failed to write known_workspaces: {e}");
        }
    }

    /// Add or update a workspace entry. The entry is identified by its path.
    /// Most-recently-used workspaces appear first in the list.
    pub fn add_or_update(&mut self, entry: KnownWorkspace) {
        if let Some(pos) = self.workspaces.iter().position(|w| w.path == entry.path) {
            self.workspaces.remove(pos);
        }
        // Insert at the front so the most recent is first.
        self.workspaces.insert(0, entry);
        // Cap the list so it doesn't grow indefinitely.
        self.workspaces.truncate(50);
        self.save();
    }

    pub fn list(&self) -> &[KnownWorkspace] {
        &self.workspaces
    }

    /// Remove workspaces whose files no longer exist on disk.
    /// Returns the number of entries removed.
    #[allow(dead_code)]
    pub fn prune_missing(&mut self) -> usize {
        let before = self.workspaces.len();
        self.workspaces.retain(|w| w.path.exists());
        let removed = before - self.workspaces.len();
        if removed > 0 {
            self.save();
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn registry_path(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("known_workspaces.json")
    }

    fn entry(name: &str, path: &str) -> KnownWorkspace {
        KnownWorkspace {
            path: PathBuf::from(path),
            display_name: name.to_string(),
            last_opened_at: "2026-05-22T00:00:00Z".to_string(),
            last_focused_at: "2026-05-22T00:00:00Z".to_string(),
            last_room_id: None,
            last_note_id: None,
            last_container_kind: None,
            last_container_id: None,
        }
    }

    #[test]
    fn load_missing_file_is_empty() {
        let dir = tempdir().unwrap();
        let reg = KnownWorkspaceRegistry::load(&registry_path(&dir));
        assert!(reg.list().is_empty());
    }

    #[test]
    fn add_and_persist() {
        let dir = tempdir().unwrap();
        let p = registry_path(&dir);
        let mut reg = KnownWorkspaceRegistry::load(&p);
        reg.add_or_update(entry("Notes", "/home/user/Notes.water"));
        assert_eq!(reg.list().len(), 1);

        let reg2 = KnownWorkspaceRegistry::load(&p);
        assert_eq!(reg2.list().len(), 1);
        assert_eq!(reg2.list()[0].display_name, "Notes");
    }

    #[test]
    fn updating_existing_moves_to_front() {
        let dir = tempdir().unwrap();
        let p = registry_path(&dir);
        let mut reg = KnownWorkspaceRegistry::load(&p);
        reg.add_or_update(entry("A", "/a.water"));
        reg.add_or_update(entry("B", "/b.water"));
        reg.add_or_update(entry("A updated", "/a.water"));

        assert_eq!(reg.list()[0].path, PathBuf::from("/a.water"));
        assert_eq!(reg.list()[0].display_name, "A updated");
        assert_eq!(reg.list().len(), 2);
    }

    #[test]
    fn prune_removes_missing() {
        let dir = tempdir().unwrap();
        let p = registry_path(&dir);
        let mut reg = KnownWorkspaceRegistry::load(&p);
        reg.add_or_update(entry("Ghost", "/nonexistent/ghost.water"));
        reg.add_or_update(entry("Real", p.to_str().unwrap())); // the registry file itself exists
        assert_eq!(reg.prune_missing(), 1);
        assert_eq!(reg.list().len(), 1);
    }
}
