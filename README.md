# Blot

Local-first notes and `.water` workspace editor for the [Watercolor suite](../watercolor-dev/WATERCOLOR_BLUEPRINT.md).

**Status: Prompt 7/12** — Place Note is live. Move an Inbox note into any workspace destination (Loose Notes, Shelf, or Pile) via the picker dialog. Entry points: editor toolbar, Desk inbox cards, Search result cards, command palette. Placement is transactional — the inbox note is only archived after the workspace copy is confirmed written.

**Watercolor/Terroir alignment:** Blot opens and edits `.water` files directly without Terroir. Terroir is optional context/indexing infrastructure; Blot keeps working when it is unavailable.

---

## What Blot Is

Blot is the main writing and note-taking surface in Watercolor. It opens directly into a blank note and saves automatically. Notes go into the private Global Inbox until placed into a `.water` workspace.

Inside a `.water` workspace, notes are organized by Rooms, Shelves, Piles, and Loose Notes.

Blot is not Obsidian, Notion, LibreOffice, or a Markdown vault. See `BLOT_BLUEPRINT.md`.

---

## Dependencies

**Runtime:**
- GTK 4.12+ (`libgtk-4-1`)
- SQLite (bundled via rusqlite)

**Build:**
- Rust 1.75+
- `libgtk-4-dev`, `pkg-config`

```sh
sudo apt install libgtk-4-dev pkg-config
```

---

## Build and Run

```sh
cargo build          # debug build
cargo build --release
cargo run            # run dev build
./target/debug/blot  # after cargo build
```

---

## Launch Options

```
blot                                    Open to blank Inbox note (default)
blot <path.water>                       Open .water workspace, switch to Workspace Mode
blot --inbox                            Open Desk / Inbox list view
blot --workspace <path.water>           Open a specific workspace
blot --search "query"                   Open in Search Mode with query prefilled
blot --room-map                         Open in Room Map Mode (stub)
blot --new-workspace-note <path.water>  Open workspace and start a new Loose Note
```

If the workspace file does not exist at the given path, Blot logs an error and opens the Inbox instead (no panic).

---

## Config and Data Paths

| Purpose | Path |
|---------|------|
| Config file | `~/.config/blot/config.toml` |
| User themes | `~/.config/blot/themes/<name>.css` |
| **Inbox database** | **`~/.local/share/blot/inbox.db`** |
| **Workspace registry** | **`~/.local/share/blot/known_workspaces.json`** |
| Cache | `~/.cache/blot/` |

---

## Inbox Behavior

- Blot opens to a blank note in the Inbox immediately on launch (no workspace picker).
- Autosave fires **1.5 seconds** after the last keystroke.
- Truly blank notes are never saved.
- On window close, any unsaved non-blank content is flushed immediately.
- Status bar shows: `Unsaved` while typing → `Saved` after each autosave.
- Smart auto-title: first heading → first meaningful line → `"Untitled note"`.
- Once the user manually edits the title, auto-titling stops for that note.

---

## Workspace Support

### .water file format

`.water` files are SQLite databases. Blot creates and manages these tables:

| Table | Purpose |
|-------|---------|
| `blot_workspace_meta` | Workspace name, schema version |
| `blot_rooms` | Rooms (top-level org units inside a workspace) |
| `blot_room_connections` | Doors between rooms (undirected, typed: normal/strong/weak) |
| `blot_shelves` | Shelves and Piles (unified table, `kind` column) |
| `notes` | Workspace notes (shared base + Blot additions: document_json, auto_titled, word_count) |
| `note_placements` | Where each note lives (room_id + shelf_id; shelf_id IS NULL = Loose) |

Schema version: 1 (Prompt 4). Migrations back up the file before altering data.

### Workspace concepts

```
Workspace (.water file)
  Room
    Shelf (intentional, named collection)
    Pile  (loose/transitional; can be converted to Shelf)
    Loose Notes (in the room, not on any shelf/pile)
  Room
    ...
```

- Every workspace has at least one Room (default: "Main Room").
- The default Room is renamable and not permanently special.
- A note lives in exactly one location: Inbox, Loose, on a Shelf, or in a Pile.
- Shelves and Piles belong to one Room.

### Creating / opening a workspace

1. Click **Desk** → **New Workspace** to create a new `.water` file via file chooser.
2. Click **Desk** → **Open…** to open an existing `.water` file.
3. Or launch: `blot /path/to/Notes.water`

### Workspace UI (Workspace Mode)

Press **Workspace** button or **Ctrl+W** to enter Workspace Mode.

Left sidebar:
- Workspace name at top
- Rooms list (click a room to select it; ✎ to rename)
- **+** button to create a new Room
- Under selected room: Shelves section, Piles section, Loose Notes section
  - Each section has a **+ Shelf** / **+ Pile** / **+ Note** button
  - Piles have a **→ Shelf** button to convert

Right panel: note editor (same as Inbox editor — autosave, smart title, source toggle).

### Autosave for workspace notes

Workspace notes autosave to the `.water` file, never to `inbox.db`. The separation is strict:
- Inbox notes → `inbox.db` only
- Workspace notes → the focused `.water` file only

### Known workspaces registry

Recently-opened workspaces are stored in `~/.local/share/blot/known_workspaces.json`. This allows Desk Mode to list workspaces without requiring Terroir. The registry tracks: path, display name, last_opened_at, last_focused_at, last_room_id, last_note_id.

---

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| Ctrl+N | New Inbox note |
| Ctrl+Shift+N | New workspace note (in current room) |
| Ctrl+D | Open Desk / Inbox list |
| Ctrl+W | Switch to Workspace Mode |
| Ctrl+F | Open Search Mode |
| Ctrl+Shift+P | Command palette |
| Escape | Back to editor |

---

## Format / Check / Test

```sh
cargo fmt
cargo check
cargo clippy
cargo test
```

Tests cover: 171 tests across title, document, inbox, workspace, known_workspaces, water_file, config, launch, desk_shell, search, and place_note modules.

Workspace-specific tests: schema initialization, default room creation, create/rename room, create shelf, create pile, convert pile to shelf, create loose note, move note to shelf and back, save/load document_json, room connections, open existing workspace, open invalid path (no panic), known workspace registry read/write.

Desk model tests: location label formatting (Inbox, Loose Notes, Shelf, Pile, Workspace), pin_location_label for inbox and workspace notes, recent_location_label with name and fallback, nonexistent path does not panic.

---

## Current Features (Prompt 7)

**Place Note (new in Prompt 7):**
- Move an Inbox note into any `.water` workspace destination via a picker dialog.
- **Entry points:** Editor toolbar "Place Note…" button; Desk inbox row "Place…" button; Search inbox result "Place…" button; Command palette "Place Note" command.
- **Picker dialog:** Workspace dropdown → Room dropdown (+ New Room) → Loose Notes / Shelf / Pile radio → container combobox (+ New Shelf / + New Pile) → suggestion hint → Place Note button.
- **Transactional placement:** workspace note is written first; inbox note is archived only after the write is confirmed.
- **Pin transfer:** if the inbox note was globally pinned, the pin is automatically transferred to the workspace note.
- **Post-placement navigation:** editor is cleared (new blank note), app switches to Workspace Mode showing the placed note.
- **Inline create:** rooms, shelves, and piles can be created from within the dialog without leaving it.
- **Suggestion:** the dialog pre-selects the last room/container used in the chosen workspace (stored in the known workspace registry).
- **Non-UI tests:** 18 tests in `src/place_note.rs` covering placement, pin transfer, recents, partial failure, double-placement prevention, document_json preservation.

**Search Mode (new in Prompt 6):**
- Full-text search over the Global Inbox and known `.water` workspaces.
- Implementation: Rust-side LIKE-equivalent filtering + multi-criteria ranking. No FTS5 yet; upgrade path is in place (`blot_search_index` spec in `BLOT_DATA_MODEL.md`).
- **Scopes:** Inbox | Current Workspace | Workspace + Inbox | All Workspaces + Inbox.
- **Filters:** Pinned only | Has Checklist | Has Image | Has Links.
- **Ranking:** title matches (2×) > body matches (1×) + whole-word bonus + start-of-title bonus. Post-score boosts: focused workspace (+5), pinned (+2), recency (+0.1–0.6 by year).
- **Result cards:** title, snippet excerpt centered on first match (160 chars + ellipsis), location breadcrumb (Workspace › Room › Shelf/Pile), date, source-kind chip, pin/checklist/image/link indicators, Open button.
- **Empty query:** shows up to 30 recent/pinned notes. Does not dump everything uncontrollably.
- **Missing workspaces:** skipped gracefully; unavailable count shown in warning banner.
- **Launch arg:** `blot --search "query"` opens Search Mode with the query pre-filled and runs the search immediately.
- **Keyboard:** Ctrl+F triggers Search Mode; Enter on selected row opens note; Escape returns to Editor.
- **Search service:** `src/search/` module — `query.rs`, `result.rs`, `snippet.rs`, `ranking.rs`, `providers.rs`. Pure Rust, no GTK dependencies. 30+ tests.

**Previous Prompts (1–5) — carried forward:**

**Desk Mode (Prompt 5):**
- Three-panel layout: Left sidebar (260 px) | Center workspace browser (hexpand) | Right Quick Actions (200 px)
- **Left panel — Inbox section:** Scrollable list of all Inbox notes, sorted newest first. Shows title, date, first-line snippet. Click to open in Editor Mode. Count badge in section header. "New Note" action button. Pinned Inbox notes show ★ indicator.
- **Left panel — Pinned section:** Notes pinned globally across all workspaces. Shows title and location (Inbox / workspace name). Click to open; switches workspace automatically if needed.
- **Left panel — Recent section:** Last 15 notes opened across all sources. Shows title, access date, location. Click to open.
- **Left panel — Workspaces section:** List of known workspaces from registry. Active workspace shown with ✓. Click to switch. "Open…" (file chooser) and "New Workspace" (file-save dialog) buttons.
- **Center panel:** When a workspace is focused, shows all Rooms with Shelves, Piles, and Loose Notes as collapsible sections. Each note shows as a card with title, date, snippet, location, pin toggle (★/☆), and "Open" button. Room header has "+ Shelf" and "+ Pile" action buttons. Pile headers have "→ Shelf" convert button. When no workspace is open, shows empty state with "Open Workspace…" and "New Workspace" buttons.
- **Right panel:** "Quick Actions" — New Inbox Note, New Workspace Note. "Workspace" section showing focused workspace name.
- **Return to Editor button** in left panel header (also Escape shortcut).
- **Global pins stored in `inbox.db`** (`blot_pins` table, schema v3). Pin/unpin any note from the center panel. UPSERT: pinning the same note twice updates metadata, not duplicate.
- **Recent notes stored in `inbox.db`** (`blot_recent` table, schema v3). Tracks last 15 accesses across Inbox and workspace notes. Updated whenever a note is opened from Desk.
- Desk refreshes all panels automatically each time it becomes visible (no stale data).

**Command palette (updated):**
- New Desk commands: "Close Desk", "Open Focused Workspace", "Switch Workspace", "Pin Current Note", "Unpin Current Note"
- Commands grouped by category with section comments

**Carried forward from Prompts 1–4:**
- Global Inbox at `~/.local/share/blot/inbox.db`
- Autosave with 1.5 s debounce; save on close
- Smart auto-title from headings / first line
- Structured block model (`NoteDocument` + `document_json`)
- Source toggle (parse ↔ serialize roundtrip)
- Command palette (Ctrl+Shift+P)
- Mode stack: Editor, Desk, Workspace, Search (stub), Room Map (stub)
- XDG-compliant paths, CSS theme, desktop file integration
- `.water` workspace: Rooms, Shelves, Piles, Loose Notes, autosave
- Known workspace registry at `~/.local/share/blot/known_workspaces.json`

---

## Desk Mode Behavior

Desk is "the memory desk behind the blank page." It appears when you press **Desk** or **Ctrl+D**.

- `blot` still opens directly into a blank Inbox note. Desk is always one keypress away.
- The Desk surface is calm and visual — not a dashboard, not VS Code's file explorer.
- Clicking any note in Desk opens it in Editor Mode, auto-saving any current note first.
- Pinning a note (★) stores a global pin in `inbox.db`. Pin state persists across sessions and is visible across Desk panels.
- Pressing Escape or "← Return to Editor" returns to the current note.

### Inbox vs. Workspace Notes

| Aspect | Inbox note | Workspace note |
|--------|-----------|----------------|
| Stored in | `~/.local/share/blot/inbox.db` | `.water` file |
| Pinnable | Yes | Yes |
| Appears in Desk Recent | Yes | Yes |
| Placeable | Yes (Prompt 7) — moves to workspace | Already in workspace |

### Global Pin Behavior

Pins are stored as rows in `blot_pins` in `inbox.db`. Each pin records:
- `target_kind`: `"inbox_note"` or `"workspace_note"`
- `target_id`: the note's ID
- `workspace_path`: empty string for inbox notes, `.water` file path for workspace notes
- `note_title`, `note_snippet`: cached display metadata (updated on re-pin)
- `sort_order`: reserved for future drag-to-reorder

UNIQUE constraint on `(target_kind, target_id, workspace_path)` prevents duplicates.

Clicking a pinned workspace note whose workspace is not currently open will open the workspace first, then the note.

### Recent Notes Behavior

Recent entries stored in `blot_recent` with UPSERT on `(target_kind, target_id, workspace_path)`. This means re-opening the same note updates its `accessed_at` and refreshes its title/snippet, rather than creating a duplicate. Up to 15 entries are shown.

### Known Limitations

- **No full-text search in Desk** — Prompt 8+. The center panel shows notes but no search box inside Desk.
- **No Room Map visual UI** — future prompt. Room connections stored in `blot_room_connections`.
- **No Arrange Mode / Compare Mode** — future prompts.
- **No Sort/Filter controls in Desk** — future prompt.
- **Right panel workspace status is static** — it shows the workspace name at Desk open time, not live-updated if you switch workspaces from within Desk.

---

## Known Limitations and Deferred Work

- **Search snippet highlighting** — text matched terms are not bolded inline (requires Pango markup); future prompt.
- **Selected-workspaces multi-select UI** — scope wired internally but not exposed as a picker in the UI yet.
- **No Room Map visual UI** — future prompt. Room connections stored in `blot_room_connections`.
- **No Arrange Mode / Compare Mode** — future prompts.
- **No Split / Merge** — future prompts.
- **FTS5 search index** — `blot_search_index` is specified but not yet created. Current search uses Rust-side filtering which is fast enough for typical note counts.
