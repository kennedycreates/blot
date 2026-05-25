/// Blot's structured note document model.
///
/// Notes are stored internally as typed block sequences (`NoteDocument`).
/// The plain text / Markdown-ish surface is a parsing/serialisation layer
/// on top, not the source of truth.
///
/// # Design rationale
///
/// Storing structured blocks rather than raw text enables future prompts to:
/// - Move blocks individually in Arrange Mode
/// - Split notes at heading/paragraph boundaries
/// - Merge notes into titled sections
/// - Attach stable IDs to blocks for bookmarks/references
/// - Embed typed objects (image cards, palette chips, thread refs, etc.)
/// - Avoid ambiguity when the same source text could mean different things
///
/// The plain text body is kept alongside `document_json` in the DB for:
/// - full-text search
/// - backward-compatible reading by older code
/// - display in the Desk note list preview
pub mod markdown;
pub mod model;
pub mod serialize;

// Public re-exports — some are forward-declared for use by later prompts.
#[allow(unused_imports)]
pub use model::{Block, BlockKind, ChecklistItem, ListItem, NoteDocument};
