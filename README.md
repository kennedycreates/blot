# Blot

Local-first notes and `.water` workspace editor for the [Watercolor suite](../watercolor-dev/WATERCOLOR_BLUEPRINT.md).

**Status: Prompt 2/12** — Global Inbox database + autosave editor active. Block engine, Search, Room Map, and `.water` workspace integration are coming in later prompts.

---

## What Blot Is

Blot is the main writing and note-taking surface in Watercolor. It opens directly into a blank note and saves automatically. Notes go into a private Global Inbox until placed into a `.water` workspace.

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
blot <path.water>                       Open a .water workspace (stub)
blot --inbox                            Open Desk / Inbox list view
blot --workspace <path.water>           Open a specific workspace (stub)
blot --search "query"                   Open in Search Mode (stub)
blot --room-map                         Open in Room Map Mode (stub)
blot --new-workspace-note <path.water>  Create a note in a workspace (stub)
```

---

## Config and Data Paths

| Purpose | Path |
|---------|------|
| Config file | `~/.config/blot/config.toml` |
| User themes | `~/.config/blot/themes/<name>.css` |
| **Inbox database** | **`~/.local/share/blot/inbox.db`** |
| Workspace registry | `~/.local/share/blot/known_workspaces.json` *(Prompt 3+)* |
| Cache | `~/.cache/blot/` |

A default `config.toml` is written on first run.

---

## Inbox Autosave Behavior

- Blot opens to a blank note in the Inbox immediately.
- Autosave fires **1.5 seconds** after the last keystroke.
- Truly blank notes (only whitespace) are never saved.
- On window close, any unsaved non-blank content is flushed immediately.
- Previously saved notes are never silently deleted, even if later emptied.
- The status bar shows: `Unsaved` while typing → `Saved` after each autosave.

## Smart Title Behavior

Auto-title resolution order:
1. First Markdown heading line (`# Title`, `## Title`, `### Title`)
2. First meaningful non-empty line of body text
3. `"Untitled note"` if all lines are structural/blank

Once the user manually edits the title field, auto-titling stops and the user's title is preserved on every subsequent save.

---

## Desk / Inbox List

Press **Desk** button or **Ctrl+D** to open the Desk view:
- Lists all Inbox notes, newest first
- Click a note row to open it in the editor (saves current note first)
- **New Note** button to start a fresh note
- Desk refreshes automatically each time it becomes visible

---

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| Ctrl+N | New Inbox note |
| Ctrl+D | Open Desk / Inbox list |
| Ctrl+F | Search (stub) |
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

Tests cover: title extraction, blank detection, word count, DB open/create, upsert, list, get, archive filtering, sort order, created_at preservation, NoteSession reset, ID uniqueness.

---

## Current Features

- Global Inbox SQLite database at `~/.local/share/blot/inbox.db`
- Schema: `inbox_notes`, `inbox_note_revisions`, `inbox_schema_version` (WAL mode)
- Autosave with 1.5 s debounce; immediate save on close
- Smart auto-title from headings / first line / fallback
- Title entry: tracks auto vs user-set; never overwrites a user-edited title
- Hint label hides when user starts typing
- Desk view: live Inbox note list, click to open, New Note button
- Stack `notify::visible-child-name` refreshes Desk each time it is shown
- Status bar: mode, location, save state
- All Prompt 1 features: command palette (24 commands), mode shells, launch arg parsing, XDG paths, CSS theme, desktop file

---

## Known Limitations Before Prompt 3

- Body stored as flat plain text, not structured blocks. Prompt 3 introduces the block document model.
- No FTS5 search index yet (planned for Search Mode prompt).
- No word count display in the status bar (trivial to add; deferred to keep Prompt 2 focused).
- Inbox note deletion not yet wired in the UI (conservative — no data loss).
- `.water` workspace integration is a stub.
# blot
