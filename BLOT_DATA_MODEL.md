# Blot Data Model — v0.5 (Prompt 8)

This document describes the data entities and storage model for Blot.

Blot uses two distinct stores:

1. **Inbox database** — SQLite file at `~/.local/share/blot/inbox.db`. Private to Blot. Not a `.water` file. Not visible to other Watercolor apps.
2. **`.water` workspace files** — SQLite databases. Implemented as of Prompt 4. Blot owns `blot_` prefixed tables plus the shared `notes` and `note_placements` tables.

**Known workspace registry:** `~/.local/share/blot/known_workspaces.json`. Tracks recently-opened workspace paths, names, and last-open state. Does not require Terroir.

**Schema authority note for `.water`:** `WATER_FILE_FORMAT.md` provides suite-level guidance but not full DDL. The schema below is the canonical Blot `.water` schema (schema_version = 1, introduced Prompt 4). Where this document and `WATER_FILE_FORMAT.md` conflict, align with this document for Blot-owned tables (`blot_*`), and with `WATER_FILE_FORMAT.md` for suite-shared tables.

> **Note on authority:** `../watercolor-dev/WATER_FILE_FORMAT.md` has stronger rules on the shared `.water` schema. Where this document conflicts with it, align with the sibling spec. The `notes` table in particular is a shared Watercolor object — Blot must not break its shape for other apps.

---

## Inbox Store

Path: `~/.local/share/blot/inbox.db`

The Inbox is not a `.water` file. It has its own simpler schema because it only needs to support Blot's note capture and search.

### `inbox_notes` (schema version 4)

Implemented columns across all prompts:

| Column | Type | Notes |
|--------|------|-------|
| `id` | TEXT | Primary key. 32-char hex ID. |
| `title` | TEXT | Derived via auto-title or user-edited. |
| `body` | TEXT | Plain Markdown-ish text. Kept for search and Desk preview. |
| `document_json` | TEXT (nullable) | Serialised `NoteDocument` JSON. **The structured source of truth.** `NULL` for notes from before Prompt 3; re-parsed on next save. |
| `auto_titled` | INTEGER (BOOL) | 1 if title was auto-derived, 0 if user set it. |
| `created_at` | TEXT | ISO 8601 string. Never overwritten on update. |
| `updated_at` | TEXT | ISO 8601 string. Updated on every save. |
| `word_count` | INTEGER | Approximate word count for Desk display. |
| `is_pinned` | INTEGER (BOOL) | Pinned in Inbox. |
| `is_archived` | INTEGER (BOOL) | 1 if archived (hidden from normal lists). Set to 1 when a note is placed into a workspace. |
| `placed_at` | TEXT (nullable) | ISO 8601 timestamp of when the note was placed. `NULL` for unplaced notes. |
| `placed_workspace_path` | TEXT (nullable) | Absolute path to the `.water` file the note was placed into. `NULL` for unplaced notes. |
| `placed_workspace_note_id` | TEXT (nullable) | ID of the note created in the workspace. Enables navigating to the placed copy. `NULL` for unplaced notes. |
| `placed_destination_label` | TEXT (nullable) | Human-readable label like "Research › Articles (Shelf)" for display. `NULL` for unplaced notes. |

## Structured Document Model (Prompt 3)

Blot stores notes as typed block sequences internally. The plain Markdown-ish text is a parsing/serialisation layer on top — not the primary representation.

### Rationale

Structured blocks enable future features that raw text cannot support cleanly:
- Arrange Mode (drag individual blocks)
- Split Note at heading/paragraph boundary
- Merge Notes into titled sections
- Stable block IDs for bookmarks and references
- Typed embedded objects (image cards, palette chips, Kindling thread links, etc.)
- Avoiding source-of-truth ambiguity

### `NoteDocument` (in-memory and in `document_json`)

```rust
NoteDocument {
    schema_version: u32,   // 1 currently
    blocks: Vec<Block>,
}

Block {
    id: String,       // stable block ID like "blk0000000000000001"
    kind: BlockKind,  // see below
}
```

### Supported Block Types (Prompt 3)

| `BlockKind` variant | JSON `type` | Description |
|---------------------|-------------|-------------|
| `Paragraph { text }` | `paragraph` | Plain prose |
| `Heading { level, text }` | `heading` | `#`–`######` headings |
| `BulletList { items }` | `bullet_list` | Unordered list |
| `NumberedList { items }` | `numbered_list` | Ordered list |
| `Checklist { items }` | `checklist` | Checkbox list |
| `Divider` | `divider` | Horizontal rule |
| `Quote { lines }` | `quote` | Block quotation |
| `Callout { style, title, lines }` | `callout` | GFM-style callout |
| `ImageCard { alt, path }` | `image_card` | Image reference |
| `NoteLink { display, target }` | `note_link` | `[[Note Name]]` |
| `FileLink { display, path }` | `file_link` | `[Label](./path)` |
| `PaletteReference { display, palette_id }` | `palette_reference` | Palette chip (stub) |
| `KindlingThreadReference { display, thread_id }` | `kindling_thread_reference` | Stub |
| `AbacusFormulaReference { display, formula_id }` | `abacus_formula_reference` | Stub |
| `FixativeCaptureReference { display, capture_id }` | `fixative_capture_reference` | Stub |
| `Unknown { raw }` | `unknown` | Catch-all; preserves future content |

### Plain Text and document_json Relationship

On every save:
1. Body text (from the editor) is stored in `body` for search/display.
2. Body is parsed into a `NoteDocument` via `document::markdown::parse()`.
3. The document is serialised to JSON and stored in `document_json`.

On load:
1. If `document_json` is present and valid: deserialise → serialise back to text → display.
2. If `document_json` is absent or invalid: display `body` directly; re-parse on next save.

The normalise-on-load step ensures that when a note is opened after a parser improvement, it automatically gets the cleaner canonical form.

### Source Toggle Behavior

The "Source" toggle button in the editor:
- Passes the current buffer text through `parse → to_source` to normalise it.
- Shows the label "← Editor" while in source mode.
- On toggle-back, normalises again and marks the note dirty so autosave updates the DB.
- Both modes currently use the same plain `TextView`; future prompts will add a rendered block view for normal mode.

### Inbox Migration History

| Schema version | Change |
|----------------|--------|
| 1 (Prompt 2) | Base `inbox_notes` table, `inbox_note_revisions`, `inbox_schema_version` |
| 2 (Prompt 3) | Added `document_json TEXT` column via `ALTER TABLE` |
| 3 (Prompt 5) | Added `blot_pins` and `blot_recent` tables for global pin/recent tracking |
| 4 (Prompt 7) | Added 4 placement columns: `placed_at`, `placed_workspace_path`, `placed_workspace_note_id`, `placed_destination_label` |

All migrations run automatically on `InboxDb::open()`. Existing databases are upgraded in-place without data loss.
| `word_count` | INTEGER | Approximate word count for display. |

### `inbox_blocks`

| Column | Type | Notes |
|--------|------|-------|
| `id` | TEXT (UUID) | Primary key. |
| `note_id` | TEXT | FK → `inbox_notes.id`. |
| `block_type` | TEXT | See block type list in `BLOT_BLUEPRINT.md`. |
| `position` | REAL | Float ordering key. Use fractional positions to insert without renumbering. |
| `content_json` | TEXT | JSON payload for this block. Schema varies by type. |
| `created_at` | INTEGER | Unix timestamp. |
| `updated_at` | INTEGER | Unix timestamp. |

### `inbox_search` (FTS5 virtual table)

```sql
CREATE VIRTUAL TABLE inbox_search USING fts5(
    note_id UNINDEXED,
    title,
    body,
    content='inbox_notes',
    content_rowid='rowid'
);
```

Rebuild trigger fires on insert/update/delete of `inbox_notes`.

### `inbox_schema_version`

| Column | Type |
|--------|------|
| `version` | INTEGER |

Single-row table. Increment on every schema migration.

---

## Known Workspace Registry

Path: `~/.local/share/blot/known_workspaces.json`

A JSON array of `KnownWorkspace` entries. Updated on each workspace open and after each Place Note operation.

| Field | Type | Notes |
|-------|------|-------|
| `path` | PathBuf | Absolute path to the `.water` file. |
| `display_name` | String | Workspace display name (from `blot_workspace_meta`). |
| `last_opened_at` | String | ISO 8601 timestamp. Updated on each open. |
| `last_focused_at` | String | ISO 8601 timestamp. Updated when focused. |
| `last_room_id` | Option\<String\> | Last room the user worked in. Restored on re-open. |
| `last_note_id` | Option\<String\> | Last note the user had open. Restored on re-open. |
| `last_container_kind` | Option\<String\> | `"shelf"` or `"pile"` for the last container used. Added in Prompt 7. |
| `last_container_id` | Option\<String\> | ID of the last container used for Place Note suggestions. Added in Prompt 7. |

All fields use `#[serde(default)]` so older JSON files deserialise safely when new fields are added.

---

## `.water` Workspace Schema (Prompt 4, Schema Version 1)

`.water` files are SQLite databases. Blot creates and manages these tables. Table names use the `blot_` prefix to avoid collision with other Watercolor apps. The shared `notes` and `note_placements` tables are defined here because Blot is the primary note editor.

All implementations live in `src/workspace.rs`.

### Table summary

| Table | Owner | Purpose |
|-------|-------|---------|
| `blot_workspace_meta` | Blot | Workspace name, schema version, default room |
| `blot_rooms` | Blot | Rooms (top-level org units) |
| `blot_room_connections` | Blot | Doors between rooms |
| `blot_shelves` | Blot | Shelves and Piles (unified, `kind` column) |
| `notes` | Shared + Blot additions | Note content |
| `note_placements` | Blot | Where each note lives (room/shelf/pile/loose) |

### Pile-to-Shelf conversion

Converting a Pile to a Shelf updates `blot_shelves.kind = 'shelf'` in place. Notes already in the pile remain in place via `note_placements.shelf_id`. This is a meaningful user action (not a silent rename).

### Loose Notes

A note is "loose" when its `note_placements.shelf_id IS NULL`. It lives in a room but is not on any shelf or pile.

### Schema version tracking

`blot_workspace_meta.schema_version` tracks the Blot schema version. On open:
- If schema_version > app's SCHEMA_VERSION: error (user must upgrade Blot).
- If schema_version < 1: back up the file, apply schema, update version.
- If tables are missing: apply schema additively (safe for alien SQLite files).

### Migration safety

Blot creates a `.water.bak` backup before running schema migrations that alter existing data.

## `.water` Workspace Schema Extensions

### Shared Table: `notes` (from WATER_FILE_FORMAT.md)

Blot uses this table for base note metadata. It is shared with other apps.

| Column | Type | Notes |
|--------|------|-------|
| `id` | TEXT (UUID) | Stable object ID. |
| `title` | TEXT | Note title. |
| `body` | TEXT | Plain-text body for export and baseline access by other apps. |
| `created_at` | INTEGER | Unix timestamp. |
| `updated_at` | INTEGER | Unix timestamp. |

> Blot stores its block-level content in `note_blocks` (below) and writes the `body` column as flattened plain text so other apps can read notes without understanding blocks.

Blot may add columns to `notes` via ALTER TABLE if needed, but must document additions here and keep them nullable (backward-compatible).

Proposed Blot additions to `notes`:

| Column | Type | Notes |
|--------|------|-------|
| `word_count` | INTEGER | Approximate word count. |
| `auto_titled` | INTEGER (BOOL) | 1 if title was auto-derived. |
| `is_archived` | INTEGER (BOOL) | 1 if note was archived by merge or replace. Archived notes are not shown in normal lists or search. |
| `redirects_to_note_id` | TEXT | If archived, the note this was merged into. |
| `merged_into_note_id` | TEXT | Prompt 10: target note ID a merged source was folded into (nullable). |
| `merged_at` | TEXT | Prompt 10: ISO 8601 time the source was archived by a merge (nullable). |

Inbox notes (`inbox_notes`) carry the equivalent merge-tracking columns
`merged_into_kind`, `merged_into_id`, `merged_into_workspace_path`, and
`merged_at`, since an Inbox note may be merged into either another Inbox note
or a workspace note.

---

### `note_blocks`

Stores the block-level content of a note. Each row is one block.

| Column | Type | Notes |
|--------|------|-------|
| `id` | TEXT (UUID) | Primary key. Stable block ID. |
| `note_id` | TEXT | FK → `notes.id`. |
| `block_type` | TEXT | Must be a valid block type from the supported list. |
| `position` | REAL | Float ordering key within the note. |
| `content_json` | TEXT | JSON payload. Schema varies per block type (see below). |
| `created_at` | INTEGER | Unix timestamp. |
| `updated_at` | INTEGER | Unix timestamp. |

Block types and their `content_json` shapes:

| Block type | Key JSON fields |
|------------|-----------------|
| `paragraph` | `text` (rich text array) |
| `heading` | `level` (1–3), `text` (plain string) |
| `bullet_list` | `items` (array of rich text), `indent` |
| `numbered_list` | `items` (array of rich text), `start_number`, `indent` |
| `checklist` | `items` (array of `{text, checked, kindling_ref?}`) |
| `divider` | `style` (solid/dashed/blank) |
| `quote` | `text` (rich text array), `attribution` (optional) |
| `callout` | `text` (rich text array), `color_hint` (optional), `icon` (optional) |
| `image_card` | `ref_type` (fixative/file), `ref_id`, `caption`, `alt_text`, `display_size` |
| `note_link` | `target_note_id`, `target_workspace_id` (if cross-workspace), `display_title`, `link_style` |
| `file_link` | `path`, `display_name`, `file_kind`, `last_known_path` |
| `palette_reference` | `palette_id`, `display_name`, `color_hint` |
| `kindling_thread_reference` | `thread_id`, `workspace_id`, `display_title`, `thread_kind` |
| `abacus_formula_reference` | `formula_id`, `workspace_id`, `display_title`, `result_snapshot` |
| `fixative_capture_reference` | `capture_id`, `path`, `display_name`, `thumbnail_path` |

Rich text arrays are sequences of `{text, bold?, italic?, code?, link?}` objects.

---

### `note_placements`

Tracks where a note lives within the workspace (Room, Shelf, Pile, or Loose).

One row per note. A note lives in exactly one location at a time.

| Column | Type | Notes |
|--------|------|-------|
| `id` | TEXT (UUID) | Primary key. |
| `note_id` | TEXT | FK → `notes.id`. Unique (one placement per note). |
| `room_id` | TEXT | FK → `blot_rooms.id`. Always set — all notes are in a room. |
| `shelf_id` | TEXT | FK → `blot_shelves.id`. Null if loose or in pile. |
| `pile_id` | TEXT | Null if on a shelf or loose. Same table as shelf_id (see `blot_shelves`, kind column). |
| `position` | REAL | Float ordering key within the containing unit. |
| `placed_at` | INTEGER | Unix timestamp of last placement change. |

> `shelf_id` and `pile_id` both reference `blot_shelves.id`. When `blot_shelves.kind = 'shelf'`, use `shelf_id`. When `kind = 'pile'`, use `pile_id`. In practice these can be unified into one nullable FK. This is a schema decision to finalize during implementation. The key invariant: a note is at most on one shelf/pile, or loose (both null).

---

### `blot_rooms`

| Column | Type | Notes |
|--------|------|-------|
| `id` | TEXT (UUID) | Primary key. |
| `name` | TEXT | User-editable room name. |
| `description` | TEXT | Optional. |
| `atmosphere_json` | TEXT | JSON: `{background_tint, accent_color, header_style, decorative_hints}`. All nullable. |
| `map_x` | REAL | X position on the Room Map canvas (0.0 = not yet positioned; auto-laid out by Room Map). |
| `map_y` | REAL | Y position on the Room Map canvas. |
| `map_width` | REAL | Visual size hint for Room Map (reserved for future use). |
| `map_height` | REAL | Visual size hint for Room Map (reserved for future use). |
| `sort_position` | REAL | Float ordering key for sidebar list. |
| `created_at` | TEXT | ISO 8601. |
| `updated_at` | TEXT | ISO 8601. Updated when the room is renamed or its map position changes. |

Every workspace gets one Room on creation. The room is named "Main Room" and is immediately renamable.

**Room Map position:** `map_x`/`map_y` start at `0.0`. When Room Map Mode opens, if all rooms are at (0, 0), automatic circular layout is applied in memory. Positions are persisted to the DB when the user drags a room card.

**Implemented in `WorkspaceDb` (Prompt 8):**
- `list_rooms()` — includes `map_x`, `map_y`
- `get_room(id)` — includes `map_x`, `map_y`
- `update_room_map_position(room_id, x, y)`
- `room_total_note_count(room_id)` — loose + shelved/piled notes
- `room_container_count(room_id)` — shelves + piles

---

### `blot_room_connections`

Doors between rooms.

| Column | Type | Notes |
|--------|------|-------|
| `id` | TEXT | Primary key. |
| `room_a_id` | TEXT | FK → `blot_rooms.id`. Always the lexicographically smaller ID. |
| `room_b_id` | TEXT | FK → `blot_rooms.id`. |
| `connection_type` | TEXT | `normal`, `strong`, or `weak`. Validated on write. |
| `label` | TEXT | Optional, unlabeled by default. |
| `created_at` | TEXT | ISO 8601. |

Connections are **undirected**: `(room_a, room_b)` and `(room_b, room_a)` are the same connection. The application always stores `room_a_id ≤ room_b_id` lexicographically. A `UNIQUE(room_a_id, room_b_id)` constraint prevents duplicates at the DB level; the application also returns the existing connection rather than inserting a duplicate.

**Rules (enforced in `WorkspaceDb`):**
- Self-connections (both IDs the same) are rejected with `WorkspaceError::Invalid`.
- Invalid connection types (anything other than `normal`, `strong`, `weak`) are rejected.
- Duplicate connections return the existing row.

**Implemented in `WorkspaceDb` (Prompt 8):**
- `create_room_connection(a, b, type)` — validates, prevents self-loop and duplicates
- `list_room_connections()` — all connections
- `list_connections_for_room(room_id)` — connections touching a specific room (either end)
- `delete_room_connection(id)` — safe delete, does not touch rooms or notes
- `update_room_connection_type(id, type)` — validated update

---

### `blot_shelves`

Stores both Shelves (intentional) and Piles (loose/transitional). Distinguished by `kind`.

| Column | Type | Notes |
|--------|------|-------|
| `id` | TEXT (UUID) | Primary key. |
| `room_id` | TEXT | FK → `blot_rooms.id`. |
| `name` | TEXT | User-editable name. |
| `kind` | TEXT | `shelf` or `pile`. |
| `description` | TEXT | Optional. |
| `position` | REAL | Float ordering key within the room. |
| `created_at` | INTEGER | Unix timestamp. |
| `updated_at` | INTEGER | Unix timestamp. |

Converting a Pile to a Shelf updates `kind = 'shelf'`. This is a meaningful user action, not a silent rename.

---

### `note_versions` (workspace) / `inbox_note_versions` (Inbox)  — Prompt 10

Point-in-time snapshots of a note's content. The same shape exists in both
the `.water` workspace (`note_versions`, FK → `notes.id`) and the Inbox DB
(`inbox_note_versions`, FK → `inbox_notes.id`). A **Bookmark** is simply a
version row with `is_bookmark = 1`.

| Column | Type | Notes |
|--------|------|-------|
| `id` | TEXT | Primary key. |
| `note_id` | TEXT | FK → the owning note. |
| `title` | TEXT | Snapshot of the note title. |
| `body` | TEXT | Snapshot of the note body. |
| `document_json` | TEXT | Snapshot of the structured document JSON (nullable). |
| `created_at` | TEXT | ISO 8601 timestamp. |
| `reason` | TEXT | Why the version was made (`manual bookmark`, `before split`, `before merge (target)`, `before merge (source)`, `before restore`). |
| `is_bookmark` | INTEGER (BOOL) | 1 if this version is a named/important bookmark. |
| `bookmark_name` | TEXT | User-supplied or default `Bookmarked <timestamp>` name (nullable). |
| `bookmark_kind` | TEXT | `manual`, `auto`, or `system`. |
| `operation_id` | TEXT | Groups versions created by a single operation, e.g. all bookmarks made before one Merge (nullable). |

**Storage rules:** versions are *not* created per keystroke — autosave is
separate. Versions are created when the user bookmarks, and automatically
("auto-bookmarks") before each risky operation (Split, Merge, Restore, and
Compare-Mode content moves). Versions are append-only.

**Restore safety:** restoring a version first auto-bookmarks the current state
(`reason = "before restore"`), then overwrites title/body/document_json with
the snapshot and autosaves. Current content is never destroyed.

---

### `note_pins`

Pinned notes for fast access.

| Column | Type | Notes |
|--------|------|-------|
| `id` | TEXT (UUID) | Primary key. |
| `note_id` | TEXT | FK → `notes.id`. |
| `pin_context` | TEXT | `global` (appears in Desk Mode), `workspace`, or `room`. |
| `context_id` | TEXT | Workspace or room ID if pin_context is not global. Null for global pins. |
| `pinned_at` | INTEGER | Unix timestamp. |

---

### `note_links`

Tracks explicit note-to-note links (for `note_link` blocks and for relationship lookup).

| Column | Type | Notes |
|--------|------|-------|
| `id` | TEXT (UUID) | Primary key. |
| `from_note_id` | TEXT | FK → `notes.id`. |
| `to_note_id` | TEXT | Target note ID (may be in same or different workspace). |
| `to_workspace_id` | TEXT | Workspace of target note. Null if same workspace. |
| `from_block_id` | TEXT | FK → `note_blocks.id`. The block that contains the link. |
| `created_at` | INTEGER | Unix timestamp. |

---

### `note_object_links`

Tracks links from notes to non-note Watercolor objects: files, threads, formulas, captures, palettes.

| Column | Type | Notes |
|--------|------|-------|
| `id` | TEXT (UUID) | Primary key. |
| `note_id` | TEXT | FK → `notes.id`. |
| `from_block_id` | TEXT | FK → `note_blocks.id`. The block containing this reference. |
| `object_type` | TEXT | `file`, `palette`, `kindling_thread`, `abacus_formula`, `fixative_capture`, `folder`, `web_link`. |
| `object_id` | TEXT | ID of the referenced object. For files, this is the path. |
| `workspace_id` | TEXT | Owning workspace if the object is a `.water` object. Null for external references. |
| `display_title` | TEXT | Cached display name. |
| `last_verified_at` | INTEGER | Last time Blot confirmed the reference was valid. |
| `is_broken` | INTEGER (BOOL) | 1 if Blot detected the reference is broken (file moved, object deleted, etc.). |
| `created_at` | INTEGER | Unix timestamp. |

---

### `blot_search_index` (FTS5 virtual table)

Full-text search over notes in the workspace.

```sql
CREATE VIRTUAL TABLE blot_search_index USING fts5(
    note_id UNINDEXED,
    title,
    body,
    room_id UNINDEXED,
    shelf_id UNINDEXED,
    content='notes',
    content_rowid='rowid'
);
```

Updated via triggers on `notes` insert/update/delete and on `note_blocks` insert/update/delete (rebuild `body` from blocks).

For cross-workspace search, Terroir provides an index when available. Without Terroir, Blot searches the open workspace and the Inbox only.

**Prompt 6 note:** `blot_search_index` FTS5 is specified but not yet created. The Prompt 6 implementation uses `WorkspaceDb::search_notes_with_placement()` — a SQL JOIN over `notes`, `note_placements`, `blot_rooms`, and `blot_shelves` — plus Rust-side filtering and ranking. The `blot_search_index` FTS5 table is the planned upgrade path when query performance on large workspaces becomes a bottleneck. The search abstraction (`src/search/providers.rs`) is designed so the provider can be swapped to FTS5 without changing the ranking or UI layers.

---

### `blot_recent_notes`

Tracks recent note access for Desk Mode "recent notes" display.

| Column | Type | Notes |
|--------|------|-------|
| `note_id` | TEXT | FK → `notes.id`. |
| `accessed_at` | INTEGER | Unix timestamp. Updated on every note open. |

Unique on `note_id`. On conflict, update `accessed_at`.

---

### `blot_workspace_meta`

App-specific workspace metadata. One row.

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER | Primary key. Always 1. |
| `schema_version` | INTEGER | Blot schema version for this workspace. |
| `default_room_id` | TEXT | The room shown when first opening the workspace. |
| `last_open_note_id` | TEXT | Last note the user had open. Restored on reopen. |
| `loose_notes_room_id` | TEXT | Which room holds Loose Notes (usually the default room). |
| `updated_at` | INTEGER | Unix timestamp. |

---

## Inbox vs. Workspace Table Comparison

| Concept | Inbox | Workspace (.water) |
|---------|-------|--------------------|
| Notes | `inbox_notes` | `notes` (shared schema) |
| Blocks | `inbox_blocks` | `note_blocks` |
| Placement | N/A (Inbox is the placement) | `note_placements` |
| Rooms | N/A | `blot_rooms` |
| Shelves/Piles | N/A | `blot_shelves` |
| Room Connections | N/A | `blot_room_connections` |
| Pins | `inbox_notes.is_pinned` | `note_pins` |
| Bookmarks | N/A (Inbox notes don't have bookmarks) | `note_bookmarks` |
| Links | N/A | `note_links`, `note_object_links` |
| Search | `inbox_search` (FTS5) | `blot_search_index` (FTS5) |
| Recent | Not tracked separately | `blot_recent_notes` |

---

## Object ID and Reference Stability

- All IDs are UUIDs (TEXT in SQLite).
- IDs must remain stable across renames, moves, and edits.
- Block IDs are stable per block. When a block is deleted, its ID is retired.
- Note IDs survive: rename, re-title, move to new room/shelf/pile, merge (as the target), archive.
- Note IDs do NOT survive: moving a note from Inbox to Workspace (this is a new note object in `.water`). The Inbox note is deleted from `inbox.sqlite` after a copy is created in the workspace.

---

## Schema Migration

Both `inbox.sqlite` and `.water` files use a schema version table.

- `inbox_schema_version` for the Inbox.
- `blot_workspace_meta.schema_version` for workspace files.

Blot must check the schema version on open and offer migration before proceeding if needed.

Blot must back up a `.water` file before running migrations that alter note content or remove blocks.

Migrations must be logged and reversible where possible.

---

## Open Questions

1. Should `note_blocks` store delta history (append-only) or latest-only? Delta history would make version diff possible without full snapshots, but increases complexity.
2. Should checklist block items be rows in a separate table (for Kindling linking) or remain as JSON inside `note_blocks.content_json`?
3. How should cross-workspace `note_link` blocks resolve when the target workspace is not open?
4. Should `blot_shelves` use a single table for both shelves and piles, or two separate tables? Single table is simpler but a nullable `kind` enum requires care.
5. How should Blot handle concurrent access to a `.water` file (e.g., Blot and Kindling both open)?
6. Should Inbox notes be assigned a permanent workspace ID when placed, or should the original Inbox ID be discarded in favor of a new workspace-native ID?
7. How should Room atmosphere JSON be versioned if the atmosphere system evolves?
8. Should `blot_recent_notes` be stored per-device (i.e., in config/state rather than in `.water`) so it doesn't pollute shared workspace files?
