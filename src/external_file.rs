//! External plain-text / Markdown file support.
//!
//! Blot can open ordinary `.txt`, `.md`, and `.markdown` files directly, edit
//! them, save back to disk, and *absorb* them into Blot's structured note
//! system. An external file is a distinct note kind: it is **not** an Inbox
//! note and **not** a workspace note until the user explicitly absorbs it.
//!
//! This module is intentionally almost pure: detection, reading, title
//! derivation, line-ending preservation, and saving are all plain functions
//! with no GTK dependency, so they are fully unit-testable. The only impure
//! seam is [`move_to_trash`], which uses GIO/GVfs trash and is documented as
//! safe (never permanent delete).
//!
//! Safety contract:
//! - Reading a missing file returns [`ExternalFileError::NotFound`] — we never
//!   create a file as a side effect of trying to open one.
//! - Unsupported extensions are rejected up front.
//! - Oversized files are rejected rather than read into memory and risk
//!   freezing the UI.
//! - Saving uses UTF-8 and preserves the original line-ending style.
//! - Trashing never permanently deletes and never runs without an explicit
//!   user action at the call site.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Largest external file Blot will open. Files above this are refused with a
/// clear warning rather than read into memory (which could freeze the UI).
pub const MAX_EXTERNAL_FILE_BYTES: u64 = 5 * 1024 * 1024; // 5 MiB

// ── File kind ───────────────────────────────────────────────────────────────

/// The supported external file flavours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalFileKind {
    /// `.txt` / `.text` — treated as plain text.
    PlainText,
    /// `.md` / `.markdown` — treated as Markdown.
    Markdown,
}

impl ExternalFileKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::PlainText => "Plain text file",
            Self::Markdown => "Markdown file",
        }
    }
}

/// Classify a path by its extension. Returns `None` for unsupported types.
///
/// Supported: `txt`, `text`, `md`, `markdown` (case-insensitive).
/// Deliberately unsupported: `.docx`, `.odt`, `.rtf`, `.pdf`, `.html`, and
/// binary files.
pub fn classify_extension(path: &Path) -> Option<ExternalFileKind> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    match ext.as_str() {
        "txt" | "text" => Some(ExternalFileKind::PlainText),
        "md" | "markdown" => Some(ExternalFileKind::Markdown),
        _ => None,
    }
}

/// True when the path looks like a supported external file (by extension).
pub fn is_supported(path: &Path) -> bool {
    classify_extension(path).is_some()
}

// ── Line endings ──────────────────────────────────────────────────────────────

/// Line-ending style of an external file, preserved across an edit/save cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    /// Unix `\n`.
    Lf,
    /// Windows `\r\n`.
    Crlf,
}

impl LineEnding {
    /// Detect the dominant line ending in `text`. Defaults to LF when there are
    /// no line breaks (or LF is at least as common as CRLF).
    pub fn detect(text: &str) -> LineEnding {
        let crlf = text.matches("\r\n").count();
        // Count bare LFs (those not part of a CRLF).
        let total_lf = text.matches('\n').count();
        let lf_only = total_lf.saturating_sub(crlf);
        if crlf > lf_only {
            LineEnding::Crlf
        } else {
            LineEnding::Lf
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::Crlf => "\r\n",
        }
    }
}

/// Normalise any mix of CRLF/LF to LF for in-editor representation.
/// GTK text buffers use `\n` internally; we store and edit in LF and only
/// re-apply the original style on save.
pub fn normalize_to_lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Render LF editor text back to disk form using the given line ending.
pub fn apply_line_ending(lf_text: &str, ending: LineEnding) -> String {
    match ending {
        LineEnding::Lf => lf_text.to_string(),
        LineEnding::Crlf => lf_text.replace('\n', "\r\n"),
    }
}

// ── Errors ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ExternalFileError {
    /// Path does not exist (or is not a regular file).
    NotFound,
    /// Extension is not a supported plain-text / Markdown type.
    Unsupported(String),
    /// File exceeds [`MAX_EXTERNAL_FILE_BYTES`].
    TooLarge(u64),
    /// File is not valid UTF-8 (likely binary).
    NotUtf8,
    /// Lower-level I/O error.
    Io(String),
}

impl std::fmt::Display for ExternalFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "File not found."),
            Self::Unsupported(ext) => write!(
                f,
                "Blot can only open plain text and Markdown files (.txt, .md, .markdown). \
                 '{ext}' is not supported."
            ),
            Self::TooLarge(bytes) => write!(
                f,
                "File is too large to open ({} bytes, limit {} bytes).",
                bytes, MAX_EXTERNAL_FILE_BYTES
            ),
            Self::NotUtf8 => write!(f, "File is not valid UTF-8 text (it may be binary)."),
            Self::Io(e) => write!(f, "Could not read file: {e}"),
        }
    }
}

impl std::error::Error for ExternalFileError {}

// ── Open external file in memory ────────────────────────────────────────────

/// An external file loaded into memory for editing. Distinct from any DB note.
#[derive(Debug, Clone)]
pub struct ExternalFile {
    pub path: PathBuf,
    pub kind: ExternalFileKind,
    /// Content normalised to LF for in-editor editing.
    pub content: String,
    /// File name including extension, e.g. `notes.md`.
    pub original_name: String,
    /// File stem (name without extension), e.g. `notes`.
    pub stem: String,
    /// Original on-disk line-ending style, re-applied on save.
    pub line_ending: LineEnding,
    /// Whether the original ended with a trailing newline.
    pub had_trailing_newline: bool,
    /// ISO-8601 last-modified time of the file when opened, if available.
    pub original_modified_at: Option<String>,
    /// Raw modified timestamp snapshot, used to detect external changes.
    pub mtime_snapshot: Option<SystemTime>,
    /// Size in bytes when opened (informational; surfaced in diagnostics).
    #[allow(dead_code)]
    pub size_bytes: u64,
}

impl ExternalFile {
    /// Title to show in the editor immediately on open: the file name stem.
    pub fn initial_title(&self) -> String {
        if self.stem.trim().is_empty() {
            self.original_name.clone()
        } else {
            self.stem.clone()
        }
    }
}

/// Read and classify an external file, refusing missing / unsupported / oversized
/// / non-UTF-8 files. Never creates a file.
pub fn read_external_file(path: &Path) -> Result<ExternalFile, ExternalFileError> {
    let kind = classify_extension(path).ok_or_else(|| {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_else(|| "(no extension)".to_string());
        ExternalFileError::Unsupported(ext)
    })?;

    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(ExternalFileError::NotFound)
        }
        Err(e) => return Err(ExternalFileError::Io(e.to_string())),
    };

    if !meta.is_file() {
        return Err(ExternalFileError::NotFound);
    }

    let size = meta.len();
    if size > MAX_EXTERNAL_FILE_BYTES {
        return Err(ExternalFileError::TooLarge(size));
    }

    let bytes = std::fs::read(path).map_err(|e| ExternalFileError::Io(e.to_string()))?;
    let raw = String::from_utf8(bytes).map_err(|_| ExternalFileError::NotUtf8)?;

    let line_ending = LineEnding::detect(&raw);
    let had_trailing_newline = raw.ends_with('\n') || raw.ends_with('\r');
    let content = normalize_to_lf(&raw);

    let original_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mtime_snapshot = meta.modified().ok();
    let original_modified_at = mtime_snapshot.map(system_time_to_iso8601);

    Ok(ExternalFile {
        path: path.to_path_buf(),
        kind,
        content,
        original_name,
        stem,
        line_ending,
        had_trailing_newline,
        original_modified_at,
        mtime_snapshot,
        size_bytes: size,
    })
}

/// Write `editor_text` (LF form) back to `path`, restoring the original line
/// ending and trailing-newline behaviour. UTF-8 only. On failure the file is
/// left as-is and the caller keeps the editor content.
pub fn save_external_file(
    path: &Path,
    editor_text: &str,
    line_ending: LineEnding,
    ensure_trailing_newline: bool,
) -> Result<(), ExternalFileError> {
    let lf = normalize_to_lf(editor_text);
    let mut out = apply_line_ending(&lf, line_ending);
    if ensure_trailing_newline && !out.is_empty() && !out.ends_with(line_ending.as_str()) {
        out.push_str(line_ending.as_str());
    }
    std::fs::write(path, out.as_bytes()).map_err(|e| ExternalFileError::Io(e.to_string()))
}

/// True when the on-disk file's modified time differs from the snapshot taken
/// when the file was opened — i.e. it changed underneath us. Used to warn
/// before overwriting. Returns `false` when we cannot tell.
pub fn file_changed_since_open(path: &Path, snapshot: Option<SystemTime>) -> bool {
    let Some(snapshot) = snapshot else {
        return false;
    };
    match std::fs::metadata(path).and_then(|m| m.modified()) {
        Ok(current) => current != snapshot,
        Err(_) => false,
    }
}

// ── Title derivation ──────────────────────────────────────────────────────────

/// Derive a title for a note absorbed from an external file.
///
/// Resolution order (deliberately different from ordinary Inbox smart titles —
/// for files the *name* matters more than the first prose line):
/// 1. First Markdown heading in the content
/// 2. File stem / name
/// 3. First meaningful (non-blank, non-structural) line
/// 4. Timestamp fallback
pub fn derive_file_title(content: &str, stem: &str, timestamp_fallback: &str) -> String {
    if let Some(heading) = first_heading(content) {
        return truncate_to(&heading, MAX_TITLE_CHARS);
    }
    let stem = stem.trim();
    if !stem.is_empty() {
        return truncate_to(stem, MAX_TITLE_CHARS);
    }
    if let Some(line) = first_meaningful_line(content) {
        return truncate_to(&line, MAX_TITLE_CHARS);
    }
    if timestamp_fallback.is_empty() {
        "Untitled file".to_string()
    } else {
        format!("Imported {timestamp_fallback}")
    }
}

const MAX_TITLE_CHARS: usize = 80;

/// First Markdown ATX heading (`#`–`######` followed by a space) text, if any.
fn first_heading(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim_start();
        let mut hashes = 0;
        for ch in trimmed.chars() {
            if ch == '#' {
                hashes += 1;
            } else {
                break;
            }
        }
        // A valid ATX heading requires whitespace after the 1–6 leading hashes
        // (so `#tag` is a tag, not a heading).
        if (1..=6).contains(&hashes) {
            let after = &trimmed[hashes..];
            if after.starts_with(char::is_whitespace) {
                let rest = after.trim();
                if !rest.is_empty() {
                    return Some(rest.to_string());
                }
            }
        }
    }
    None
}

/// First non-blank line that is not a horizontal rule / setext underline.
fn first_meaningful_line(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("---") || trimmed.starts_with("===") {
            continue;
        }
        return Some(trimmed.to_string());
    }
    None
}

fn truncate_to(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{truncated}…")
    }
}

// ── Trash (safe move, never permanent delete) ────────────────────────────────

#[derive(Debug)]
pub enum TrashError {
    /// The file did not exist — nothing was trashed.
    NotFound,
    /// GIO/GVfs trash failed (e.g. no trash backend, permissions).
    Failed(String),
}

impl std::fmt::Display for TrashError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "File not found; nothing was moved to Trash."),
            Self::Failed(e) => write!(f, "Could not move file to Trash: {e}"),
        }
    }
}

/// Move a file to the system Trash via GIO/GVfs. This never permanently
/// deletes. If the file is missing it returns `NotFound` rather than erroring
/// destructively. The caller must only invoke this after an explicit user
/// action ("Move to Trash").
pub fn move_to_trash(path: &Path) -> Result<(), TrashError> {
    use gio::prelude::FileExt;
    if !path.exists() {
        return Err(TrashError::NotFound);
    }
    let file = gio::File::for_path(path);
    file.trash(gio::Cancellable::NONE)
        .map_err(|e| TrashError::Failed(e.to_string()))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn system_time_to_iso8601(t: SystemTime) -> String {
    let secs = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    glib::DateTime::from_unix_local(secs)
        .and_then(|dt| dt.format_iso8601())
        .map(|s| s.to_string())
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn write_file(dir: &tempfile::TempDir, name: &str, content: &[u8]) -> PathBuf {
        let path = dir.path().join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content).unwrap();
        path
    }

    // ── Detection ───────────────────────────────────────────────────────────

    #[test]
    fn txt_is_supported() {
        assert_eq!(
            classify_extension(Path::new("/x/a.txt")),
            Some(ExternalFileKind::PlainText)
        );
        assert!(is_supported(Path::new("a.txt")));
    }

    #[test]
    fn md_is_supported() {
        assert_eq!(
            classify_extension(Path::new("a.md")),
            Some(ExternalFileKind::Markdown)
        );
        assert_eq!(
            classify_extension(Path::new("a.markdown")),
            Some(ExternalFileKind::Markdown)
        );
    }

    #[test]
    fn text_extension_supported() {
        assert_eq!(
            classify_extension(Path::new("a.text")),
            Some(ExternalFileKind::PlainText)
        );
    }

    #[test]
    fn extension_is_case_insensitive() {
        assert_eq!(
            classify_extension(Path::new("README.MD")),
            Some(ExternalFileKind::Markdown)
        );
        assert_eq!(
            classify_extension(Path::new("NOTES.TXT")),
            Some(ExternalFileKind::PlainText)
        );
    }

    #[test]
    fn unsupported_extensions_rejected() {
        for name in ["a.docx", "a.pdf", "a.rtf", "a.odt", "a.html", "a.png"] {
            assert!(
                classify_extension(Path::new(name)).is_none(),
                "{name} should be unsupported"
            );
            assert!(!is_supported(Path::new(name)));
        }
    }

    #[test]
    fn no_extension_unsupported() {
        assert!(classify_extension(Path::new("/x/PLAINFILE")).is_none());
    }

    // ── Read ──────────────────────────────────────────────────────────────────

    #[test]
    fn reads_txt_file() {
        let dir = tempdir().unwrap();
        let path = write_file(&dir, "note.txt", b"Hello world\nSecond line\n");
        let ef = read_external_file(&path).unwrap();
        assert_eq!(ef.kind, ExternalFileKind::PlainText);
        assert_eq!(ef.content, "Hello world\nSecond line\n");
        assert_eq!(ef.original_name, "note.txt");
        assert_eq!(ef.stem, "note");
        assert!(ef.had_trailing_newline);
        assert_eq!(ef.line_ending, LineEnding::Lf);
    }

    #[test]
    fn reads_md_file() {
        let dir = tempdir().unwrap();
        let path = write_file(&dir, "doc.md", b"# Title\n\nBody");
        let ef = read_external_file(&path).unwrap();
        assert_eq!(ef.kind, ExternalFileKind::Markdown);
        assert!(!ef.had_trailing_newline);
    }

    #[test]
    fn missing_file_handled_gracefully() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("does-not-exist.txt");
        assert!(matches!(
            read_external_file(&path),
            Err(ExternalFileError::NotFound)
        ));
        // Crucially, no file was created.
        assert!(!path.exists());
    }

    #[test]
    fn unsupported_file_rejected_on_read() {
        let dir = tempdir().unwrap();
        let path = write_file(&dir, "a.pdf", b"%PDF-1.4");
        assert!(matches!(
            read_external_file(&path),
            Err(ExternalFileError::Unsupported(_))
        ));
    }

    #[test]
    fn non_utf8_rejected() {
        let dir = tempdir().unwrap();
        let path = write_file(&dir, "bin.txt", &[0xff, 0xfe, 0x00, 0x01]);
        assert!(matches!(
            read_external_file(&path),
            Err(ExternalFileError::NotUtf8)
        ));
    }

    #[test]
    fn crlf_detected_and_normalised() {
        let dir = tempdir().unwrap();
        let path = write_file(&dir, "win.txt", b"line one\r\nline two\r\n");
        let ef = read_external_file(&path).unwrap();
        assert_eq!(ef.line_ending, LineEnding::Crlf);
        assert_eq!(ef.content, "line one\nline two\n");
    }

    // ── Save round-trip ─────────────────────────────────────────────────────

    #[test]
    fn save_preserves_lf() {
        let dir = tempdir().unwrap();
        let path = write_file(&dir, "n.txt", b"original\n");
        save_external_file(&path, "edited\ncontent", LineEnding::Lf, true).unwrap();
        let back = std::fs::read_to_string(&path).unwrap();
        assert_eq!(back, "edited\ncontent\n");
    }

    #[test]
    fn save_preserves_crlf() {
        let dir = tempdir().unwrap();
        let path = write_file(&dir, "n.txt", b"x");
        save_external_file(&path, "a\nb", LineEnding::Crlf, false).unwrap();
        let back = std::fs::read(&path).unwrap();
        assert_eq!(back, b"a\r\nb");
    }

    #[test]
    fn save_round_trips_through_read() {
        let dir = tempdir().unwrap();
        let path = write_file(&dir, "rt.md", b"# H\r\n\r\nbody\r\n");
        let ef = read_external_file(&path).unwrap();
        // Edit in LF, save back preserving CRLF.
        save_external_file(&path, &ef.content, ef.line_ending, ef.had_trailing_newline).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\r\n"));
        let ef2 = read_external_file(&path).unwrap();
        assert_eq!(ef2.content, ef.content);
    }

    // ── External change detection ─────────────────────────────────────────────

    #[test]
    fn unchanged_file_not_flagged() {
        let dir = tempdir().unwrap();
        let path = write_file(&dir, "c.txt", b"hi");
        let ef = read_external_file(&path).unwrap();
        assert!(!file_changed_since_open(&path, ef.mtime_snapshot));
    }

    // ── Title derivation ──────────────────────────────────────────────────────

    #[test]
    fn title_prefers_heading() {
        assert_eq!(
            derive_file_title("# My Heading\nbody", "filename", "2026-06-01"),
            "My Heading"
        );
        assert_eq!(
            derive_file_title("### Deep\nbody", "filename", "2026-06-01"),
            "Deep"
        );
    }

    #[test]
    fn title_falls_back_to_stem_before_first_line() {
        // No heading → stem wins over the first prose line (file name matters).
        assert_eq!(
            derive_file_title("just some prose\nmore", "meeting-notes", "2026-06-01"),
            "meeting-notes"
        );
    }

    #[test]
    fn title_falls_back_to_first_line_when_no_stem() {
        assert_eq!(
            derive_file_title("first meaningful line\nmore", "", "2026-06-01"),
            "first meaningful line"
        );
    }

    #[test]
    fn title_timestamp_fallback() {
        assert_eq!(
            derive_file_title("\n\n   \n", "", "2026-06-01"),
            "Imported 2026-06-01"
        );
    }

    #[test]
    fn title_skips_horizontal_rule_for_first_line() {
        assert_eq!(
            derive_file_title("---\nreal content", "", "ts"),
            "real content"
        );
    }

    #[test]
    fn hash_without_space_is_not_heading() {
        // "#tag" is not an ATX heading; with no stem we fall to first line.
        assert_eq!(derive_file_title("#tag here", "", "ts"), "#tag here");
    }

    // ── Trash safety ──────────────────────────────────────────────────────────

    #[test]
    fn trash_missing_file_returns_not_found_without_side_effects() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ghost.txt");
        assert!(matches!(move_to_trash(&path), Err(TrashError::NotFound)));
    }

    #[test]
    fn line_ending_detect_defaults_to_lf() {
        assert_eq!(LineEnding::detect("no breaks"), LineEnding::Lf);
        assert_eq!(LineEnding::detect("a\nb\nc"), LineEnding::Lf);
        assert_eq!(LineEnding::detect("a\r\nb\r\n"), LineEnding::Crlf);
    }

    #[test]
    fn initial_title_uses_stem() {
        let dir = tempdir().unwrap();
        let path = write_file(&dir, "shopping-list.txt", b"milk\neggs");
        let ef = read_external_file(&path).unwrap();
        assert_eq!(ef.initial_title(), "shopping-list");
    }
}
