# Blot Roadmap — v0.1

This roadmap describes the implementation plan for Blot.

The goal is not a toy MVP. The first serious build of Blot should include all major concepts: the Inbox, block-based editing, workspaces with Rooms/Shelves/Piles, Room Map, Desk Mode, Search Mode, and the command palette. Integrations with sibling apps may begin as stubs in early phases but should not be removed or blocked out.

Read `../watercolor-dev/ROADMAP.md` for suite-level context. Blot is Phase 5 in the Watercolor roadmap, building after the Palette model and `.water` format are stable.

---

## Prerequisites (Before Coding)

Before writing Blot application code, these sibling specs should be stable:

- `../watercolor-dev/WATER_FILE_FORMAT.md` — shared `.water` SQLite schema
- `../watercolor-dev/PALETTE_MODEL.md` — palette concept and membership
- `../watercolor-dev/TERROIR_CONTRACT.md` — Terroir integration interface

Blot may begin with stubs for Palette and Terroir integration, but the schema extensions it adds to `.water` must not conflict with those specs.

---

## Phase 1: Project Foundation

**Goal:** Working Rust/GTK4 project with a blank note and autosave.

### 1.1 Project Skeleton

- Initialize Rust project in `blot-dev/`.
- Add GTK4 dependencies via gtk-rs.
- Set up `cargo fmt`, `cargo clippy`, `cargo test` as standard dev commands.
- Configure XDG paths: `~/.config/blot/`, `~/.cache/blot/`, `~/.local/share/blot/`.
- Create a basic GTK4 application window with header bar and status bar.

### 1.2 Inbox Database

- Initialize `~/.local/share/blot/inbox.sqlite` on first launch.
- Implement `inbox_notes`, `inbox_blocks`, `inbox_search` (FTS5), and `inbox_schema_version` tables as specified in `BLOT_DATA_MODEL.md`.
- Schema migration mechanism with version checks on open.

### 1.3 Basic Editor (Inbox note)

- Open directly into a blank Inbox note on launch.
- No picker, no dialog.
- Paragraph blocks only at this stage.
- Autosave every edit with debounce.
- Auto-title on first save from: first heading → first meaningful line → timestamp.
- Discard truly blank notes silently.
- Display note title (large, editable) above the content area.
- Status bar: shows "Inbox", word count, save state.

### 1.4 Block Model Core

- Implement the internal block representation in Rust (enum or struct-based).
- Block types for Phase 1: `paragraph`, `heading`, `divider`.
- Position ordering via float key.
- Serialize/deserialize blocks to `content_json`.
- Write flattened `search_text` to `inbox_notes` on each save.

**Phase 1 deliverable:** Blot opens, user writes, note is autosaved to Inbox. Title is derived automatically. Window shows correct status bar info.

---

## Phase 2: Full Block Type Set and Editor Completeness

**Goal:** All initial block types work in the editor with inline Markdown-ish shortcuts.

### 2.1 Remaining Block Types

Implement editing and rendering for:
- `bullet_list`
- `numbered_list`
- `checklist` (local only; Kindling integration comes later)
- `quote`
- `callout`
- `divider` (done in 1.4, polish here)
- `image_card` (file-based images; Fixative integration comes later)
- `note_link` (within Inbox for now; workspace note links come later)
- `file_link` (reference only; open action comes later)
- `palette_reference` (stub display; Palette integration comes later)
- `kindling_thread_reference` (stub display)
- `abacus_formula_reference` (stub display)
- `fixative_capture_reference` (stub display)

### 2.2 Inline Block Creation

- `/` command inline: type `/` on an empty line to open a block insertion menu. Filter by typing.
- Markdown-ish shortcut conversion on input (see `BLOT_UI_MODEL.md`).
- Block menu via left-handle: click to open block type/action menu for that block.

### 2.3 Formatting Toolbar

- Context toolbar appears on text selection.
- Bold, italic, code, link actions.
- Convert selected text to a callout or quote.

### 2.4 Raw Source Toggle

- Toggle button (or command palette) shows the note as Markdown-like plain text.
- Edits in raw view parse back to blocks on toggle-off.
- Warn if raw edit creates ambiguous block structure.

### 2.5 Block Reordering (Drag and Drop)

- Block handles on hover.
- Drag to reorder within the note.

### 2.6 Inbox Note List

- Simple note list (left sidebar or a Desk-like list view).
- Shows title, first line snippet, date.
- Click to open a note.
- New note button.

**Phase 2 deliverable:** Full block type set. User can write rich notes in the Inbox with inline shortcuts, a formatting toolbar, and drag-to-reorder.

---

## Phase 3: Workspace Support (.water Files)

**Goal:** Blot opens and creates `.water` workspace files. Notes can exist in workspaces.

### 3.1 .water File Initialization

- Open existing `.water` files via file picker or command line.
- Create new `.water` workspace files.
- Apply Blot's schema extensions to `.water` on open (add `blot_` tables if not present).
- Schema version tracking via `blot_workspace_meta`.

### 3.2 Room, Shelf, Pile Entities

- Implement `blot_rooms`, `blot_shelves`, `note_placements` as specified in `BLOT_DATA_MODEL.md`.
- Default Room created on new workspace.
- Create Rooms, Shelves, Piles via UI and command palette.
- Convert Pile to Shelf (with confirmation).

### 3.3 Workspace Note Editing

- Create new notes in a workspace (inside a room, on a shelf/pile, or loose).
- Edit notes using the same Editor Mode as Inbox notes.
- Save notes to the `notes` and `note_blocks` tables in the `.water` file.
- Write plain-text `body` column to `notes` for other apps.

### 3.4 Note Placement

- Inbox note → Place Note flow: pick workspace, room, shelf/pile (or loose). Create destinations inline.
- Note disappears from Inbox after placement.
- `note_placements` row created.

### 3.5 Breadcrumb and Location Display

- Status bar and note header show: Workspace › Room › Shelf/Pile.
- Breadcrumb is clickable to navigate.

### 3.6 Workspace Note List

- Left sidebar shows: current room's shelves, piles, and loose notes.
- Click to open a note.

**Phase 3 deliverable:** Blot opens `.water` files, creates rooms/shelves/piles, places Inbox notes into workspaces, and edits workspace notes. Full block model works in workspaces.

---

## Phase 4: Desk Mode

**Goal:** Full Desk Mode with Inbox, recents, pins, workspace navigation, and quick actions.

### 4.1 Desk Mode Layout

- Trigger from command palette or keyboard/button.
- Sections: Inbox panel, Pinned Notes, Recent Notes, Current Workspace panel, Workspaces list, Quick Search.
- All sections visible in one view without excessive scrolling.

### 4.2 Pinned Notes

- Pin notes at global, workspace, or room scope.
- `note_pins` table in workspace; pinned flag in Inbox.
- Pinned notes shown in Desk Mode and optionally in editor sidebar.

### 4.3 Recent Notes

- `blot_recent_notes` table tracks note access.
- Recent list in Desk Mode shows: title, workspace, room, time.

### 4.4 Workspace List in Desk Mode

- Show known workspaces (from config, from recent files, and from Terroir when available).
- Open workspace, create workspace, pin workspace actions.

### 4.5 Tabs

- Tab bar in header.
- Each tab is an independent editor/mode context.
- Tabs remember scroll and cursor position.
- Drag tabs to new windows.

### 4.6 Multiple Windows

- Open new window from menu or tab drag.
- All windows share the same Inbox and workspace data.

**Phase 4 deliverable:** Desk Mode is fully functional. Tabs and multiple windows work.

---

## Phase 5: Search Mode

**Goal:** Fast, rich search with all result metadata, scope controls, and filters.

### 5.1 FTS5 Search (Workspace)

- `blot_search_index` FTS5 virtual table in `.water`.
- Rebuild index on note insert/update/delete.
- Search returns: note ID, title snippet, body snippet.

### 5.2 FTS5 Search (Inbox)

- `inbox_search` FTS5 virtual table in `inbox.sqlite`.
- Search results from Inbox included when scope includes Inbox.

### 5.3 Search Result Rows

- Each result shows: title, snippet, date edited, workspace, room, shelf/pile, palette chip, pin indicator, bookmark indicator, linked-object icons, image thumbnail.
- Matched terms highlighted in title and snippet.

### 5.4 Scope Controls

- Inbox only
- Current workspace
- Selected workspaces
- All known workspaces (stub; full Terroir integration later)

### 5.5 Filters

- By room, shelf/pile, palette, date range, block type presence, has image, has links.

### 5.6 Keyboard Navigation in Search

- Up/down moves through results.
- Enter opens selected note.
- Escape closes search.

**Phase 5 deliverable:** Search Mode fully functional with rich result display, scope control, and keyboard navigation.

---

## Phase 6: Room Map Mode

**Goal:** Visual and list view of Rooms and Doors within a workspace.

### 6.1 Room Map Canvas

- Rooms displayed as labeled cards on a scrollable/zoomable canvas.
- `map_x`, `map_y`, `map_width`, `map_height` from `blot_rooms`.
- Drag rooms on the canvas to rearrange; positions saved.

### 6.2 Doors Rendering

- Draw lines between connected rooms.
- Line style reflects connection type: normal (solid), strong (bold solid), weak (dashed).
- Click a connection line to view/edit connection type or delete the door.

### 6.3 Room Map Interactions

- Right-click room: Rename, Add Door, Remove, View Notes.
- Add Door: click source room, then target room, then pick connection type.
- Double-click room to navigate into it (switch to workspace note list for that room).

### 6.4 Room Map List/Sidebar View

- Toggle between canvas and list view.
- List view: room name, note count, shelf/pile count, connections listed as text.

### 6.5 Room Map Creation

- "Create Room" from Room Map Mode.
- New room appears on canvas at a default position, ready to rename.

**Phase 6 deliverable:** Room Map Mode shows rooms and doors visually and as a list. All room and door CRUD works.

---

## Phase 7: Arrange Mode and Compare Mode

**Goal:** Intentional structural editing and two-note comparison.

### 7.1 Arrange Mode

- Enter via command palette or mode toggle.
- Blocks shown with large drag handles.
- Heading sections grouped as movable cards (heading + following paragraphs until next heading).
- Drag to reorder.
- Up/Down toolbar buttons for keyboard ordering.
- "Extract to new note" → triggers Split Note flow.
- Auto-bookmark before entering Arrange Mode.
- "Done" returns to Editor Mode.

### 7.2 Split Note

- User selects content (or positions cursor at a heading/paragraph boundary).
- Trigger "Split Note" from command palette.
- Selected content moves to a new note.
- A `note_link` block is left in place.
- Auto-bookmark fires on original note before split.
- New note opens in a new tab.

### 7.3 Merge Notes

- Trigger "Merge Notes" from command palette.
- User selects source notes.
- Each source becomes a titled section appended to the target.
- Source notes are archived, `is_archived = 1`, `redirects_to_note_id` set.
- Auto-bookmark fires on target before merge.

### 7.4 Compare Mode

- Two-panel layout.
- Pick two notes (current note as Note A, user picks Note B).
- Select blocks in Note A → Copy to Note B or Move to Note B.
- Auto-bookmark before any move.
- Confirm dialog before destructive move.

**Phase 7 deliverable:** Arrange, Split, Merge, and Compare all work correctly with auto-bookmarks.

---

## Phase 8: Bookmarks and Version History

**Goal:** Manual and auto bookmarks, visible version history tucked away.

### 8.1 Manual Bookmarks

- "Bookmark Version" in command palette.
- User names the bookmark.
- `note_bookmarks` row created with full block snapshot.

### 8.2 Auto-Bookmarks

- Auto-bookmark fires before: Split, Merge, Absorb, large Arrange edits.
- Label: `Auto before Split`, `Auto before Merge`, etc.
- `is_auto = 1`.

### 8.3 Version History Panel

- Accessible from Right Info Panel or command palette "Show Version History".
- Lists bookmarks by date: label, date, auto/manual indicator.
- Click to preview a bookmark (read-only side panel).
- "Restore this version" replaces current blocks with snapshot (auto-bookmark of current state first).
- Delete bookmark removes the snapshot.

**Phase 8 deliverable:** Full bookmark/version history system. Auto-bookmark fires on all risky operations.

---

## Phase 9: Absorb .txt / .md

**Goal:** Plain text and Markdown file absorption into Blot note format.

### 9.1 File Open / Absorb Flow

- When the user opens a `.txt` or `.md` file (via file picker or command line argument):
  - Show a dialog: "Edit as file" or "Absorb into Blot".
- "Edit as file": open as a plain text editing buffer. Blot does not convert or import.
- "Absorb": convert file to block model. Then ask: "Leave original file" or "Move to Trash".

### 9.2 Markdown Parser for Absorb

- Parse headings (`#`, `##`, `###`) → `heading` blocks.
- Parse `-`, `*`, `+` lists → `bullet_list` blocks.
- Parse `1.` lists → `numbered_list` blocks.
- Parse `- [ ]`, `- [x]` → `checklist` blocks.
- Parse `>` → `quote` blocks.
- Parse `---` → `divider` blocks.
- Parse remaining text → `paragraph` blocks.
- Best-effort; complex Markdown (tables, footnotes, HTML) degrades gracefully to `paragraph`.

### 9.3 Post-Absorb Placement

- Absorbed note goes to Inbox first.
- User can then place it into a workspace via normal Place Note flow.

**Phase 9 deliverable:** Absorb flow works for `.txt` and common `.md` patterns. User controls what happens to the original file.

---

## Phase 10: Palette Integration

**Goal:** Notes can be attached to Palettes. Palette visual inheritance works.

### 10.1 Palette Attachment

- "Attach Palette" in command palette.
- Picks from known palettes (from Terroir if available; from current `.water` file otherwise).
- Attaches palette to note: `note_object_links` row with `object_type = 'palette'`.
- Palette chip appears below note title.

### 10.2 Visual Inheritance from Palette

- Notes attached to a Palette show that palette's color/tint as a chip.
- If the Palette has tag/shape/tint settings, notes in the workspace display those markers in list views and search results.

### 10.3 `palette_reference` Block

- Users can embed a `palette_reference` block inline in a note body.
- Block displays as a small named chip with palette color.
- Click opens the palette (in Blot's palette view or hands off to Terroir).

### 10.4 Palette View in Blot (Stub)

- A minimal palette view within Blot: lists notes attached to the selected palette across the open workspace.
- Full palette cross-app board view is Lattice and Terroir territory.

**Phase 10 deliverable:** Palette attachment works. Visual inheritance displays correctly. `palette_reference` blocks render. Basic in-Blot palette view is available.

---

## Phase 11: Terroir Integration

**Goal:** Blot uses Terroir when available for workspace discovery, cross-workspace search, and relationship context.

### 11.1 Terroir Discovery

- On startup, check if Terroir is running (local API or D-Bus; TBD per `TERROIR_CONTRACT.md`).
- If available: populate workspace list from Terroir.
- If unavailable: use recent files from `~/.config/blot/recent_workspaces.toml`.

### 11.2 Cross-Workspace Search via Terroir

- When Terroir is available and scope is "All Known Workspaces", route search through Terroir.
- Results merged with local results. Terroir results include workspace metadata.

### 11.3 Relationship Context from Terroir

- Right Info Panel "Related" section (when Terroir is available): shows objects in other apps that reference this note.
- Examples: a Kindling thread that links to this note; a Fixative capture attached to this note in another workspace.

### 11.4 Broken Reference Detection

- Blot periodically checks `note_object_links` for broken references (file moved, note deleted, etc.).
- Broken references marked `is_broken = 1`.
- If Terroir is available, ask Terroir for repair suggestions on broken file references.
- Show a small indicator on `file_link` blocks that are broken.

### 11.5 Graceful Degradation

- All features from Phase 1–10 must work without Terroir.
- Phase 11 features are enhancements, not requirements. Blot must never show a hard error because Terroir is unavailable.

**Phase 11 deliverable:** Blot integrates with Terroir for discovery, search, relationship context, and broken reference repair. Degrades gracefully when Terroir is not running.

---

## Phase 12: Cross-App Integration Stubs → Real Integration

**Goal:** Block references to Kindling, Abacus, and Fixative objects work as real links.

### 12.1 Kindling Thread References

- `kindling_thread_reference` blocks display thread title and status (if Kindling data is available in the same `.water` file).
- "Open in Kindling" button on the block.
- Offer to convert a `checklist` block into a Kindling thread (prompts user, creates thread in `.water`, replaces checklist with a `kindling_thread_reference`).

### 12.2 Abacus Formula References

- `abacus_formula_reference` blocks display formula title and last result snapshot.
- "Open in Abacus" button on the block.
- Accept formula results pasted/sent from Abacus into the block.

### 12.3 Fixative Capture References

- `fixative_capture_reference` and `image_card` blocks referencing Fixative captures display the capture thumbnail.
- "Open in Fixative" button.
- Accept captures attached from Fixative via the Attach Image / Absorb flow.

### 12.4 File Links

- `file_link` blocks: "Open" launches the file in its associated app.
- "Open in Lattice" option opens the file's location in Lattice if Lattice is available.

**Phase 12 deliverable:** All cross-app block reference types are functional (not just stubs). Opening linked objects in their apps works.

---

## Phase 13: Export

**Goal:** First-class note and workspace export.

### 13.1 Single Note Export

- Export current note as: plain text, Markdown, JSON (block model).
- "Export Note" in command palette.
- File picker for save location.

### 13.2 Multi-Note Export

- "Export All Notes" exports all notes in the current workspace.
- Output: folder of `.txt` or `.md` files, one per note.
- File names derived from note titles.

### 13.3 Workspace Export

- Full workspace export: notes + room structure metadata + palette attachments.
- Output: folder with notes and a `workspace_meta.json`.

**Phase 13 deliverable:** Users can export their Blot data to portable formats. No one is trapped in `.water`.

---

## Phase 14: Polish, Accessibility, and Packaging

**Goal:** Blot is ready for real daily use and installable.

### 14.1 Accessibility Pass

- Full keyboard navigation through all modes.
- Visible focus states.
- No color-only information conveyance.
- Font size follows system preferences.

### 14.2 Empty States

- All empty states implemented with helpful text per `BLOT_UI_MODEL.md`.

### 14.3 Visual Coherence

- Room atmosphere rendering polished.
- Palette chip display polished.
- Status bar, breadcrumbs, and all metadata displays consistent with Watercolor design system.

### 14.4 Performance

- Large notes (500+ blocks) remain usable in Editor and Arrange modes.
- Search results return in under 200ms for local workspaces.

### 14.5 Packaging

- `.desktop` file, icon, MIME association for `.water`.
- `watercolor-blot` package following `../watercolor-dev/PACKAGING.md`.
- Install/uninstall documentation.

**Phase 14 deliverable:** Blot is stable, accessible, coherent, and installable as `watercolor-blot`.

---

## Deferred (Not in Roadmap)

These are explicitly deferred. Do not add them without user instruction.

- High-contrast theme (planned post-Phase 14)
- PDF export
- Print layout / page-based formatting
- Real-time collaboration
- Cloud sync
- Plugin/extension system
- AI writing assistance
- Full outliner view (not Blot's model)
- Backlinks as a primary navigation mode
- Database tables inside notes (Notion-style)
- Embedded spreadsheets
- Mobile version
