use std::path::PathBuf;

/// Parsed launch configuration for the main Blot window.
#[derive(Debug, Clone, Default)]
pub struct LaunchConfig {
    /// Open a specific .water workspace on startup.
    pub workspace: Option<PathBuf>,
    /// Open directly to the Inbox view.
    pub inbox: bool,
    /// Open the Room Map for the given workspace instead of the editor.
    pub room_map: bool,
    /// Pre-fill the search query and open in Search Mode.
    pub search_query: Option<String>,
    /// Create a new note in the given workspace on startup.
    pub new_workspace_note: Option<PathBuf>,
    /// Open an external plain-text / Markdown file (`.txt`, `.md`, `.markdown`,
    /// `.text`) directly in Editor Mode.
    pub external_file: Option<PathBuf>,
}

impl LaunchConfig {
    pub fn from_args(args: &[String]) -> Self {
        let mut config = LaunchConfig::default();
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--inbox" => {
                    config.inbox = true;
                    i += 1;
                }
                "--workspace" => {
                    if i + 1 < args.len() {
                        config.workspace = Some(PathBuf::from(&args[i + 1]));
                        i += 2;
                    } else {
                        eprintln!("blot: --workspace requires a path argument");
                        i += 1;
                    }
                }
                "--room-map" => {
                    config.room_map = true;
                    i += 1;
                }
                "--search" => {
                    if i + 1 < args.len() {
                        config.search_query = Some(args[i + 1].clone());
                        i += 2;
                    } else {
                        eprintln!("blot: --search requires a query argument");
                        i += 1;
                    }
                }
                "--new-workspace-note" => {
                    if i + 1 < args.len() {
                        config.new_workspace_note = Some(PathBuf::from(&args[i + 1]));
                        i += 2;
                    } else {
                        eprintln!("blot: --new-workspace-note requires a workspace path");
                        i += 1;
                    }
                }
                "--file" => {
                    if i + 1 < args.len() {
                        config.external_file = Some(PathBuf::from(&args[i + 1]));
                        i += 2;
                    } else {
                        eprintln!("blot: --file requires a file path");
                        i += 1;
                    }
                }
                arg if !arg.starts_with('-') => {
                    // Positional arg: a supported plain-text / Markdown file opens
                    // as an external file; anything else is treated as a .water
                    // workspace path (preserving Prompt 1-10 behavior).
                    let path = PathBuf::from(arg);
                    if crate::external_file::is_supported(&path) {
                        config.external_file = Some(path);
                    } else {
                        config.workspace = Some(path);
                    }
                    i += 1;
                }
                arg => {
                    eprintln!("blot: unknown argument '{arg}'");
                    i += 1;
                }
            }
        }
        config
    }
}

/// Top-level launch decision. Currently there is only one launch mode
/// for Blot; the enum exists as an extension point for future modes
/// (e.g. a dedicated capture mode or system integration helper).
#[derive(Debug)]
pub enum LaunchMode {
    Normal(LaunchConfig),
}

impl LaunchMode {
    /// Parse the process environment arguments into a launch mode.
    pub fn from_env() -> Self {
        let args: Vec<String> = std::env::args().skip(1).collect();
        Self::Normal(LaunchConfig::from_args(&args))
    }

    pub fn config(&self) -> &LaunchConfig {
        let Self::Normal(config) = self;
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn default_launch_is_clean() {
        let config = LaunchConfig::from_args(&[]);
        assert!(config.workspace.is_none());
        assert!(!config.inbox);
        assert!(!config.room_map);
        assert!(config.search_query.is_none());
        assert!(config.new_workspace_note.is_none());
        assert!(config.external_file.is_none());
    }

    #[test]
    fn parses_inbox_flag() {
        let config = LaunchConfig::from_args(&args(&["--inbox"]));
        assert!(config.inbox);
    }

    #[test]
    fn parses_workspace_flag() {
        let config = LaunchConfig::from_args(&args(&["--workspace", "/tmp/notes.water"]));
        assert_eq!(config.workspace, Some(PathBuf::from("/tmp/notes.water")));
    }

    #[test]
    fn positional_arg_becomes_workspace() {
        let config = LaunchConfig::from_args(&args(&["/home/user/Notes.water"]));
        assert_eq!(
            config.workspace,
            Some(PathBuf::from("/home/user/Notes.water"))
        );
    }

    #[test]
    fn parses_search_flag() {
        let config = LaunchConfig::from_args(&args(&["--search", "my query"]));
        assert_eq!(config.search_query.as_deref(), Some("my query"));
    }

    #[test]
    fn parses_room_map_flag() {
        let config = LaunchConfig::from_args(&args(&["--room-map"]));
        assert!(config.room_map);
    }

    #[test]
    fn unknown_flags_do_not_panic() {
        let config = LaunchConfig::from_args(&args(&["--nonexistent-flag"]));
        assert!(config.workspace.is_none());
    }

    #[test]
    fn positional_txt_becomes_external_file() {
        let config = LaunchConfig::from_args(&args(&["/home/user/notes.txt"]));
        assert_eq!(
            config.external_file,
            Some(PathBuf::from("/home/user/notes.txt"))
        );
        assert!(config.workspace.is_none());
    }

    #[test]
    fn positional_md_becomes_external_file() {
        let config = LaunchConfig::from_args(&args(&["/home/user/README.md"]));
        assert_eq!(
            config.external_file,
            Some(PathBuf::from("/home/user/README.md"))
        );
    }

    #[test]
    fn positional_water_is_still_workspace() {
        let config = LaunchConfig::from_args(&args(&["/home/user/Notes.water"]));
        assert_eq!(
            config.workspace,
            Some(PathBuf::from("/home/user/Notes.water"))
        );
        assert!(config.external_file.is_none());
    }

    #[test]
    fn parses_file_flag() {
        let config = LaunchConfig::from_args(&args(&["--file", "/tmp/a.md"]));
        assert_eq!(config.external_file, Some(PathBuf::from("/tmp/a.md")));
    }
}
