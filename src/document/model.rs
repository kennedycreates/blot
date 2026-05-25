/// Blot's internal structured note document model.
///
/// Notes are stored as a list of typed `Block`s. This enables future features
/// like Arrange Mode, Split, Merge, block-level links, and palette references
/// without relying on raw Markdown text as the source of truth.
///
/// The user-facing editing surface (plain text / Markdown-ish) is an import/
/// export layer on top of this model, not the primary representation.
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

pub const CURRENT_VERSION: u32 = 1;

// ── Document ──────────────────────────────────────────────────────────────────

/// A complete structured note.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteDocument {
    /// Schema version — bump when the model changes incompatibly.
    pub schema_version: u32,
    pub blocks: Vec<Block>,
}

impl NoteDocument {
    pub fn new(blocks: Vec<Block>) -> Self {
        NoteDocument {
            schema_version: CURRENT_VERSION,
            blocks,
        }
    }

    /// Text of the first heading block, for use as the auto-title.
    pub fn first_heading_text(&self) -> Option<&str> {
        for block in &self.blocks {
            if let BlockKind::Heading { text, .. } = &block.kind {
                let t = text.trim();
                if !t.is_empty() {
                    return Some(t);
                }
            }
        }
        None
    }

    /// Whether the document has no meaningful content.
    #[allow(dead_code)] // used in tests; will be used by the editor in later prompts
    pub fn is_empty(&self) -> bool {
        self.blocks.iter().all(|b| b.is_blank())
    }
}

// ── Block ─────────────────────────────────────────────────────────────────────

/// A single structured content unit within a note.
///
/// The `id` is stable across edits so future prompts can reference blocks by
/// ID for Arrange Mode, Split, Merge, and bookmarks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub id: String,
    #[serde(flatten)]
    pub kind: BlockKind,
}

impl Block {
    pub fn new(kind: BlockKind) -> Self {
        Block {
            id: new_block_id(),
            kind,
        }
    }

    /// True when the block contributes no visible content.
    #[allow(dead_code)]
    pub fn is_blank(&self) -> bool {
        match &self.kind {
            BlockKind::Paragraph { text } => text.trim().is_empty(),
            BlockKind::Heading { text, .. } => text.trim().is_empty(),
            BlockKind::BulletList { items } => items.is_empty(),
            BlockKind::NumberedList { items } => items.is_empty(),
            BlockKind::Checklist { items } => items.is_empty(),
            BlockKind::Quote { lines } => lines.iter().all(|l| l.trim().is_empty()),
            BlockKind::Callout { lines, .. } => lines.iter().all(|l| l.trim().is_empty()),
            BlockKind::Unknown { raw } => raw.trim().is_empty(),
            _ => false, // structural blocks (Divider, ImageCard, links) always count
        }
    }
}

// ── BlockKind ─────────────────────────────────────────────────────────────────

/// The type and content of a single block.
///
/// Serialises with `type` as the tag field so JSON is human-readable:
/// `{"type": "paragraph", "text": "hello"}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BlockKind {
    /// Plain prose.
    Paragraph { text: String },

    /// Markdown heading.  `level` is 1–6 matching `#`–`######`.
    Heading { level: u8, text: String },

    /// Unordered list.
    BulletList { items: Vec<ListItem> },

    /// Ordered (numbered) list.
    NumberedList { items: Vec<ListItem> },

    /// Checkbox/task list.
    Checklist { items: Vec<ChecklistItem> },

    /// Horizontal rule / section separator.
    Divider,

    /// Block quotation.  `lines` are the quoted lines without the `> ` prefix.
    Quote { lines: Vec<String> },

    /// GFM-style callout (`> [!note] Title`).
    Callout {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        lines: Vec<String>,
    },

    /// Image reference.
    ImageCard { alt: String, path: String },

    /// `[[Target Note Name]]` — link to another Blot note.
    NoteLink { display: String, target: String },

    /// `[Label](./path)` — link to a file on disk.
    FileLink { display: String, path: String },

    /// Visual chip linking to a Watercolor Palette object.
    PaletteReference { display: String, palette_id: String },

    /// Stub block referencing a Kindling thread (Prompt 12+).
    KindlingThreadReference { display: String, thread_id: String },

    /// Stub block referencing an Abacus formula (Prompt 12+).
    AbacusFormulaReference { display: String, formula_id: String },

    /// Stub block referencing a Fixative capture (Prompt 12+).
    FixativeCaptureReference { display: String, capture_id: String },

    /// Catch-all for blocks from unknown future versions or unrecognised
    /// syntax.  Preserved in `raw` so no data is lost.
    Unknown { raw: String },
}

// ── List items ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListItem {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecklistItem {
    pub text: String,
    pub checked: bool,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Generate a stable, unique block ID without external crates.
pub fn new_block_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("blk{n:016x}")
}
