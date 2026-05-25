use std::path::PathBuf;

/// Resolved XDG-compliant data, config, and cache paths for Blot.
///
/// All paths follow the XDG Base Directory Specification.
/// Create required directories on first run with `ensure_dirs()`.
#[allow(dead_code)] // inbox_db and known_workspaces used in Prompt 2+
pub struct AppPaths {
    /// `~/.config/blot/`
    pub config_dir: PathBuf,
    /// `~/.local/share/blot/`
    pub data_dir: PathBuf,
    /// `~/.cache/blot/`
    pub cache_dir: PathBuf,
    /// `~/.config/blot/config.toml`
    pub config_file: PathBuf,
    /// `~/.config/blot/themes/`  — user CSS theme overrides
    pub themes_dir: PathBuf,
    /// `~/.local/share/blot/inbox.db`  — Inbox SQLite database (Prompt 2)
    pub inbox_db: PathBuf,
    /// `~/.local/share/blot/known_workspaces.json`  — workspace registry (Prompt 2+)
    pub known_workspaces: PathBuf,
}

impl AppPaths {
    /// Resolve all paths from the current XDG environment.
    pub fn resolve() -> Self {
        let config_dir = glib::user_config_dir().join("blot");
        let data_dir = glib::user_data_dir().join("blot");
        let cache_dir = glib::user_cache_dir().join("blot");

        AppPaths {
            config_file: config_dir.join("config.toml"),
            themes_dir: config_dir.join("themes"),
            inbox_db: data_dir.join("inbox.db"),
            known_workspaces: data_dir.join("known_workspaces.json"),
            config_dir,
            data_dir,
            cache_dir,
        }
    }

    /// Create all required directories. Logs errors but does not panic.
    pub fn ensure_dirs(&self) {
        for dir in [
            &self.config_dir,
            &self.themes_dir,
            &self.data_dir,
            &self.cache_dir,
        ] {
            if let Err(e) = std::fs::create_dir_all(dir) {
                eprintln!("blot: could not create directory {}: {e}", dir.display());
            }
        }
    }
}
