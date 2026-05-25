# Blot Blueprint — v0.1

Blot is the text editor and `.water` workspace editor in the Watercolor suite.

---

## What Blot Is

Blot is a local-first Linux app for writing, notes, thought capture, and workspace organization inside `.water` files.

It is one of the hearts of Watercolor. If a user wants to write something down, Blot is the first place they go.

Blot is not a fancy document editor. It is not a wiki. It is not a Markdown vault. It is a calm, direct writing surface that knows how to connect to the rest of Watercolor.

---

## Core Product Philosophy

### Write First, Organize Later

Blot opens into a blank note. No workspace picker. No title dialog. No template. No folder. The user is writing in under one second.

Organization is available, encouraged, and powerful — but it comes after capture, not before.

### The Inbox Is a Safe Quarantine Zone

Blot keeps a private Global Inbox that lives outside any `.water` file. Inbox notes are:

- Autosaved immediately
- Searchable
- Private to Blot (invisible to Lattice, Kindling, Abacus, Fixative, and Terroir until placed)
- Not the permanent home for anything

The Inbox should feel safe and fast. It should gently encourage placement over time, not shame the user for leaving things there.

### Notes Have a Structured Internal Model

Notes are stored as structured blocks, not raw text. The user experience may feel Markdown-ish and a raw source toggle exists — but internally, Blot operates on a block tree. This makes operations like Split, Merge, Arrange, and embedded references reliable.

### Workspaces Have Spatial Organization

Inside a `.water` workspace, Blot organizes notes using Rooms, Shelves, Piles, and Loose Notes. This structure is designed to match how people actually think about related groups of writing:

- Rooms are distinct areas of a workspace (a project, a domain, a mood)
- Shelves hold intentional, named collections of notes
- Piles hold loose, unresolved clusters of notes
- Loose Notes are in a workspace but not yet placed on any shelf or pile

---

## Key Concepts

### Global Inbox

The Global Inbox is a private Blot-level store that lives at `~/.local/share/blot/inbox.sqlite`.

Properties:
- Notes in the Inbox are invisible to all other Watercolor apps until placed.
- Notes are autosaved on every meaningful edit.
- The Inbox is always available even without any `.water` workspace open.
- The Inbox has its own search scope.
- The Inbox is a temporary home. Blot should make it easy and natural to place notes into workspaces.

The Inbox is not a Pile. It is not inside any `.water` file. It is not a workspace.

### `.water` Workspaces

A `.water` file is a SQLite-backed Watercolor workspace. Blot is the primary editor for notes inside `.water` files.

A workspace may contain many notes, organized across Rooms, Shelves, and Piles.

Blot usually focuses on one workspace at a time. Search can optionally expand to the Inbox, selected workspaces, or all known workspaces.

A user may have:
- One large `Kennedy.water` for everything
- Several focused workspaces (`Kids Cook.water`, `Game Design.water`, etc.)
- A hybrid

Blot must handle all of these.

### Rooms

A Room is the top-level organizational unit inside a workspace.

Properties:
- Every workspace has at least one Room.
- The first Room is renamable and not permanently special.
- Rooms have an atmosphere (visual/mood settings that notes in the room inherit).
- Rooms contain Shelves, Piles, and Loose Notes.
- Rooms can connect to each other through Doors.

Rooms are not folders. They are not pages. They are more like areas of a workspace or studio.

### Doors (Room Connections)

Rooms can connect to each other through Doors.

Properties:
- Doors are unlabeled by default.
- Connection types: normal, strong, weak.
- Doors are shown in Room Map Mode (visual and list views).

Doors express relationships between areas of work. A strong door might mean "these rooms are closely related." A weak door might mean "there's some connection but I'm not sure yet."

### Shelves

A Shelf is an intentional, named collection of notes within a Room.

Properties:
- Belongs to exactly one Room.
- Has a name.
- Notes on a Shelf are in a deliberate order or grouping.
- Shelves are for organized material.

### Piles

A Pile is a loose, messy, transitional cluster of notes within a Room.

Properties:
- Belongs to exactly one Room.
- Has a name (optional or auto-named).
- Notes in a Pile have not been deliberately organized yet.
- A Pile can be converted into a Shelf (this is a meaningful upgrade operation, not just a rename).

Piles are not bad. They are a natural landing zone for things that need sorting later.

### Loose Notes

A Loose Note is in a workspace but not placed on any Shelf or Pile.

Properties:
- Belongs to a workspace.
- Is inside a Room (all workspace notes are in a Room).
- Not on any Shelf or Pile yet.

Loose Notes are visible in a workspace's Loose Notes list and can be searched like any other note.

### Note Placement

A note lives in exactly one location at a time:

- In the Inbox (outside .water)
- Loose in a Room (inside .water, no shelf/pile)
- On a Shelf (inside .water)
- In a Pile (inside .water)

The "Place Note" operation moves an Inbox note into a workspace. The user can specify room, shelf, or pile inline, or drop it in Loose.

### Auto-Titles

When a note is first saved, Blot derives a title:

1. First heading block if one exists
2. First meaningful line of text (not a blank line, not a divider)
3. Timestamp fallback: `Note — May 21 2026 2:14 PM`

The title can be edited at any time. Auto-title only runs once on first save (or on demand).

Truly blank notes are discarded silently. Any note with content must be saved.

### Visual Inheritance

Notes inherit visual identity from two sources:

- **Room atmosphere** — controls the mood/environment of notes in that room (background tone, header color, decorative elements)
- **Palette tints/shapes/tags** — when a note is attached to a Palette, it inherits the palette's visual meaning markers

Room controls atmosphere. Palette controls meaning.

These two systems are independent and additive.

---

## Block Types

Notes are composed of blocks. The initial supported block types are:

| Type | Description |
|------|-------------|
| `paragraph` | Standard text paragraph |
| `heading` | Section heading, levels 1–3 |
| `bullet_list` | Unordered list |
| `numbered_list` | Ordered list |
| `checklist` | Checkbox list (can link to Kindling) |
| `divider` | Visual separator line |
| `quote` | Block quotation |
| `callout` | Highlighted callout with optional icon/color |
| `image_card` | Embedded image with optional caption (references Fixative capture or file) |
| `note_link` | Inline reference to another Blot note |
| `file_link` | Reference to a file or folder |
| `palette_reference` | Visual chip linking to a Palette |
| `kindling_thread_reference` | Reference to a Kindling thread or procedure |
| `abacus_formula_reference` | Reference to an Abacus formula or calculation |
| `fixative_capture_reference` | Reference to a Fixative capture |

Block types will expand over time. This is the initial set for v1.

---

## Core Operations

### Place Note

Move an Inbox note into a workspace.

- User selects a destination workspace, room, shelf, or pile.
- Destinations can be created inline during the Place Note dialog.
- Note disappears from Inbox after placement.

### Split Note

Divide a note into two notes at a selection boundary.

- Selected content (paragraph, heading section, checklist group, image card, callout, divider) moves into a new note.
- A `note_link` block is left in the original note where the content was.
- Auto-bookmark runs before the split.

### Merge Notes

Combine multiple source notes into a target note.

- Each source note becomes a titled section (using the source note's title as a heading) appended to the target.
- Source notes are archived and redirected to the target.
- Auto-bookmark runs on the target before the merge.

### Bookmarks

Bookmarks are named snapshots of a note's content at a point in time.

- Users can create manual bookmarks with a label.
- Blot creates automatic bookmarks before: Split, Merge, Absorb, and large Arrange edits.
- Version history is a tucked-away feature. It should not dominate normal UI.
- Bookmarks let the user restore a previous version of a note.

### Absorb .txt / .md

When a user opens a plain text or Markdown file in Blot, Blot offers a choice:

1. **Edit as file** — Blot treats it as a plain text file and does not import it.
2. **Absorb** — Blot converts the file's content into a block-model note.

On Absorb, Blot asks:
- **Leave the original file in place** — Blot keeps a copy of the content as a note; the file remains on disk.
- **Move original to Trash** — Blot takes ownership; the file is trashed.

Absorb should handle Markdown syntax (headings → heading blocks, `- ` → bullet blocks, `- [ ]` → checklist blocks, etc.).

---

## Integration with the Watercolor Suite

Blot integrates with sibling apps through `.water` files, Terroir, and documented block references. It does not import sibling app internals.

### Terroir

Terroir provides:
- Known workspace discovery (the workspace list Blot shows)
- Cross-workspace search
- Relationship context (what's related to this note)
- Broken reference repair suggestions

Blot must function without Terroir. Without Terroir, Blot loses: global search across workspaces, relationship suggestions, and auto-discovery of workspace files. Users can still open `.water` files directly.

### Kindling

Blot can:
- Embed `kindling_thread_reference` blocks in notes.
- Offer to convert a `checklist` block into a Kindling thread (a future operation, not v1).

### Abacus

Blot can:
- Embed `abacus_formula_reference` blocks in notes.
- Display formula results inline (future, not v1).

### Fixative

Blot can:
- Embed `image_card` blocks that reference Fixative captures.
- Accept images from Fixative via file reference or attach action.

### Lattice

Blot can:
- Embed `file_link` blocks referencing files managed by Lattice.
- Open files in their native app via the `file_link` block action.

### Palettes

Notes can be attached to Palettes. A note can carry a `palette_reference` block inline. Palette attachment affects visual inheritance (tints/shapes) but does not move the note anywhere.

---

## User Workflows

### Quick Capture

1. Open Blot (it opens to a blank note in the Inbox).
2. Write.
3. Close or switch away — note is autosaved with an auto-title.
4. Later: open Desk Mode, see Inbox notes, place or ignore them.

### Writing in a Workspace

1. Open or create a `.water` workspace from Desk Mode.
2. Navigate to a Room, Shelf, or Pile.
3. Create a new note there, or find an existing one.
4. Write in Editor Mode.
5. Use Arrange Mode to restructure large notes.
6. Use Split to break a note into pieces if it grows too large.

### Organizing a Pile

1. Open a Room in Room Map Mode or Desk Mode.
2. See the Piles in that room.
3. Open a Pile and drag notes to a Shelf, or convert the Pile itself into a Shelf.

### Searching

1. Trigger Search Mode.
2. Type a query.
3. Results show: title, snippet, date edited, workspace, room, shelf/pile, palette indicators, pins, bookmarks, linked object icons, image thumbnails.
4. Narrow scope to Inbox only, current workspace, selected workspaces, or all known workspaces.

### Comparing Two Notes

1. Open Compare Mode (command palette or menu).
2. Select a second note.
3. See both notes side by side.
4. Copy or move content blocks between them.

---

## What Blot Explicitly Does Not Do

- No cloud sync or cloud storage.
- No real-time collaboration.
- No Markdown files as primary storage format.
- No Markdown vault browser or graph mind map as a primary surface.
- No full document formatting (margins, page breaks, print layout).
- No team management, project statuses, or sprints.
- No per-note sharing or publishing.
- No plugin or extension system.
- No forced template for new notes.
- No mandatory workspace selection before writing.
