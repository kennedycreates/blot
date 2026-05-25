use crate::paths::AppPaths;
use std::path::Path;

/// Application configuration loaded from `~/.config/blot/config.toml`.
/// Falls back to sensible defaults for missing or malformed fields.
#[derive(Clone, Debug)]
pub struct AppConfig {
    /// Name of the CSS theme file in `~/.config/blot/themes/<name>.css`.
    /// `"default"` loads Blot's bundled theme.
    pub theme: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: "default".to_string(),
        }
    }
}

impl AppConfig {
    /// Load config from the resolved paths. Writes an example config on
    /// first run. Falls back to defaults if the file is missing or malformed.
    pub fn load(paths: &AppPaths) -> Self {
        if !paths.config_file.exists() {
            write_example_config(&paths.config_file);
            return Self::default();
        }

        let Ok(content) = std::fs::read_to_string(&paths.config_file) else {
            eprintln!(
                "blot: could not read config file {}",
                paths.config_file.display()
            );
            return Self::default();
        };

        parse_config(&content)
    }
}

fn parse_config(content: &str) -> AppConfig {
    let mut config = AppConfig::default();
    for raw_line in content.lines() {
        let line = strip_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"');
        match key {
            "theme" if !value.is_empty() => config.theme = value.to_string(),
            _ => {}
        }
    }
    config
}

fn strip_comment(line: &str) -> &str {
    let mut in_quote = false;
    for (idx, ch) in line.char_indices() {
        match ch {
            '"' => in_quote = !in_quote,
            '#' if !in_quote => return &line[..idx],
            _ => {}
        }
    }
    line
}

fn write_example_config(path: &Path) {
    let Some(parent) = path.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let _ = std::fs::write(path, EXAMPLE_CONFIG);
}

const EXAMPLE_CONFIG: &str = r#"# Blot configuration
# Generated on first run. Existing config files are not rewritten automatically.

# Theme name. "default" loads Blot's bundled theme.
# Other names load ~/.config/blot/themes/<name>.css when present.
theme = "default"
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_theme() {
        let config = parse_config(r#"theme = "calm""#);
        assert_eq!(config.theme, "calm");
    }

    #[test]
    fn ignores_comment_lines() {
        let config = parse_config("# theme = \"dark\"\ntheme = \"light\"");
        assert_eq!(config.theme, "light");
    }

    #[test]
    fn ignores_inline_comments() {
        let config = parse_config(r#"theme = "soft" # override here"#);
        assert_eq!(config.theme, "soft");
    }

    #[test]
    fn defaults_on_empty_input() {
        let config = parse_config("");
        assert_eq!(config.theme, "default");
    }

    #[test]
    fn ignores_unknown_keys() {
        let config = parse_config("unknown_key = \"value\"\ntheme = \"x\"");
        assert_eq!(config.theme, "x");
    }
}
