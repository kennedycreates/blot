# Blot UI Model — v0.2 (Prompt 7)

This document describes Blot's visual structure, modes, and interactive surfaces.

Read `BLOT_BLUEPRINT.md` for product context. Read `../watercolor-dev/DESIGN_SYSTEM.md` for suite-wide visual language.

---

## Design Principles for Blot

- **Writing surface is primary.** Sidebars, panels, and chrome serve the writing surface — not the other way around.
- **Mouse-first, keyboard-capable.** Every action has a visible mouse path. Keyboard shortcuts are convenient, not required.
- **Modes are full transitions, not overlays toggled with a tiny button.** The user should clearly know which mode they are in.
- **Command palette is always available.** It is the universal escape hatch when the user can't find a button.
- **Calm by default.** UI chrome collapses when the user is writing. It comes back when the user asks.

## Visual Theme

Blot's default theme should visually belong beside Lattice.

Use Lattice's dark Watercolor palette as the default app chrome:

- charcoal and near-black backgrounds
- aged cream primary text
- muted vellum/brown secondary text
- brass, rose, teal, blue, and violet accents
- subtle gradients, inset highlights, and fine separators

Blot should still feel quieter than Lattice because writing is the main task. Borrow the palette, button states, sidebars, cards, and status/header treatments, but do not reshape Blot into a file-manager layout.

The editor itself is dark by default. It keeps generous writing margins and a calm centered column rather than using dense cards for the note body.

---

## Window Structure

Blot is a native GTK4 app. It supports tabs and multiple windows.

### Window Zones

```
┌─────────────────────────────────────────────────────────┐
│ Header Bar                                               │
│  [App Menu]  [Tab Bar]             [Window controls]     │
├──────────┬──────────────────────────────────┬───────────┤
│          │                                  │           │
│ Sidebar  │       Content Area               │  Info     │
│ (Left)   │       (Mode-dependent)           │  Panel    │
│          │                                  │ (Right,   │
│          │                                  │ optional) │
│          │                                  │           │
├──────────┴──────────────────────────────────┴───────────┤
│ Status Bar (mode, workspace, room, word count, save)     │
└─────────────────────────────────────────────────────────┘
```

**Header Bar:** App menu, tab bar, mode indicator, window controls. Minimal. Does not grow.

**Left Sidebar:** Context-dependent. Collapses in Editor Mode. Shows workspace/room navigation in most other modes.

**Content Area:** The main working surface. Changes with mode.

**Right Info Panel:** Optional. Shows note metadata, linked objects, palette attachments, bookmarks. Off by default in Editor Mode. Can be toggled.

**Status Bar:** Always visible. Shows current mode, workspace name, room name, shelf/pile, word count, autosave state.

---

## Tabs

Blot supports multiple tabs per window. Each tab is an independent editor context.

- Tabs may be notes, the Desk, the Room Map, or Search.
- Tabs have titles (note title, or mode name).
- Tabs can be dragged to new windows.
- Tabs remember scroll position and cursor location.
- Closing a tab does not close the note. Notes are autosaved.

---

## Multiple Windows

Blot supports multiple top-level windows.

- Each window is fully independent.
- All windows share the same Inbox and workspace data.
- Compare Mode can be opened as a two-panel split within one window, or as two separate windows.

---

## Modes

Blot has six major modes plus the command palette overlay.

### 1. Editor Mode (Default)

The default calm writing surface.

**When active:**
- Left sidebar collapses or shows a minimal navigator (room/shelf breadcrumb, note list).
- Content area is the note body, full-width with generous margins.
- Block-level editing. User types like in any text editor; heading syntax, `- ` for bullets, `- [ ]` for checklists are recognized and converted in real time.
- Formatting toolbar appears contextually on text selection (bold, italic, code, link, convert to callout).
- Block type can be changed via the block menu (click the block's left handle or `/` command inline).
- Inline `/` triggers a block insertion menu (type to filter block types).

**UI elements:**
- Note title at the top (editable, large, styled as H0-equivalent).
- Block handles on hover (left of each block): drag to reorder, click for block menu.
- Palette chip below title if note is attached to a palette.
- Room + shelf/pile breadcrumb trail (small, above title or in status bar).
- Word count in status bar.
- Autosave indicator (quiet: "Saved a moment ago" or similar).
- Toggle button for Right Info Panel.
- Command palette button (or keyboard trigger).
- **"Place Note…" button** in the editor toolbar (right of breadcrumb). Disabled until the note is saved at least once. Opens the Place Note picker dialog.

**Markdown-ish shortcuts recognized inline:**
- `# ` → heading level 1
- `## ` → heading level 2
- `### ` → heading level 3
- `- ` or `* ` → bullet list
- `1. ` → numbered list
- `- [ ] ` → checklist item
- `---` on a blank line → divider
- `> ` → quote block
- ` ``` ` → code block (future)

**Raw source toggle:** Available in the block menu or command palette. Shows the full note as Markdown-like plain text. Edits in raw view are parsed and converted back to blocks on toggle-off.

**Auto-title:**
- On first save after any content is entered, Blot sets the title from the first heading, first meaningful line, or timestamp.
- Title shown above the note in large editable text at all times.

---

### 2. Desk Mode

Desk Mode is "the memory desk behind the blank page." It is a full-window surface for finding, reopening, and organizing notes — not a corporate dashboard.

`blot` still opens into Editor Mode by default. Desk is one click (or Ctrl+D) away.

#### Layout: Three-Panel

```
┌──────────────────────┬────────────────────────────────┬───────────────┐
│ LEFT (260 px fixed)  │ CENTER (fills remaining width) │ RIGHT (200 px)│
│                      │                                │               │
│ ← Return to Editor   │  [Workspace Name]  [+ New Note]│ Quick Actions │
│ ──────────────────── │  ─────────────────────────────│  New Inbox Note│
│ Inbox (N)  [New Note]│                                │  New WS Note  │
│  [note list]         │  Room: Research                │ ─────────────│
│ ──────────────────── │    📚 Articles (3) · Shelf     │ Workspace     │
│ Pinned               │      [note card]               │  [name]       │
│  [pin list]          │      [note card]               │               │
│ ──────────────────── │    📦 Drafts (1) · Pile [→Shelf│               │
│ Recent               │      [note card]               │               │
│  [recent list]       │    Loose Notes (2)             │               │
│ ──────────────────── │      [note card]               │               │
│ Workspaces           │                                │               │
│  [ws list]           │                                │               │
│  [Open…] [New WS]    │                                │               │
└──────────────────────┴────────────────────────────────┴───────────────┘
```

#### Left Panel

**Return to Editor** — button at top. Returns to the current note (also Escape).

**Inbox section**
- All non-archived Inbox notes, newest first. Placed (archived) notes are hidden.
- Section header shows count: "Inbox (N)".
- "New Note" action button in header.
- Clicking a row opens the note in Editor Mode, saving current note first.
- ★ Pinned indicator shown for notes that are globally pinned.
- **"Place…" button** on each inbox note row. Opens the Place Note picker dialog.

**Pinned section**
- Notes pinned globally from any source (Inbox or any workspace).
- Clicking opens the note; if its workspace is not currently open, it switches first.
- Shows location as "Inbox" or "workspace-name · Workspace".

**Recent section**
- Last 15 notes accessed from Desk, newest first.
- Updated each time you open a note via Desk (Open button, row click).
- Shows title, access date, and location label.

**Workspaces section**
- Known workspaces from `~/.local/share/blot/known_workspaces.json`.
- Active workspace shown with ✓.
- Clicking a row opens that workspace and switches to Workspace Mode.
- "Open…" button: file chooser for `.water` files.
- "New Workspace" button: file-save dialog to create a new `.water` file.

#### Center Panel

When a workspace is focused, shows all Rooms with their contents:
- **Room header**: "Room: [name]" + "+ Shelf" and "+ Pile" action buttons.
- **Shelf section**: 📚 icon, name, note count. Notes shown as cards. 
- **Pile section**: 📦 icon, name, note count. "→ Shelf" convert button on header. Notes shown as cards.
- **Loose Notes section**: shows count, "+ New" button to create a loose note.

Each note is shown as a **note card** with:
- Top row: title (bold), date (right-aligned), ★/☆ pin toggle button.
- Bottom row: snippet (italic, truncated), location label (right-aligned), "Open" button.
- Clicking "Open" saves the current note, tracks a recent entry, and opens the note in Editor Mode (Workspace Mode).

When no workspace is open, center shows an empty state with "Open Workspace…" and "New Workspace" buttons.

#### Right Panel (Quick Actions)

- **New Inbox Note**: create a fresh Inbox note, switch to Editor Mode.
- **New Workspace Note**: create a note in the focused workspace's current room, switch to Workspace Mode. Disabled (tooltip) when no workspace is open.
- **Workspace** section: shows focused workspace name.

#### Global Pins

Pins are stored in `blot_pins` in `inbox.db` (schema v3). Each pin records:
- `target_kind`: `"inbox_note"` or `"workspace_note"`
- `target_id`: note ID
- `workspace_path`: empty for inbox notes, `.water` path for workspace notes
- `note_title`, `note_snippet`: cached display metadata (updated on re-pin)

UNIQUE on `(target_kind, target_id, workspace_path)`. Pinning the same note twice refreshes metadata, not duplicates. Pins survive app restarts.

#### Recent Notes

Stored in `blot_recent` in `inbox.db` (schema v3). UPSERT on `(target_kind, target_id, workspace_path)` — re-opening the same note updates its timestamp and metadata rather than adding a new row. The 15 most recent entries are shown.

#### Known Limitations (Prompt 7)

- **No full-text search in Desk** — Prompt 6+.
- **No Sort/Filter controls** — Prompt 6+.
- **Right panel workspace name is static** — shown at Desk-open time; does not auto-refresh if the workspace changes.
- **Room Map, Arrange Mode, Compare Mode** — future prompts.

---

### 3. Search Mode (Implemented — Prompt 6)

The central discovery moment in Blot.

**Trigger:**
- Header bar "Search" button
- Keyboard: Ctrl+F
- Command palette: "Search" or "Search All Workspaces"
- Launch arg: `blot --search <query>` (opens Search Mode with query prefilled)

**Search bar:** Prominent, focused automatically on mode entry.

**Default scope:** If a `.water` workspace is focused, defaults to Current Workspace. Otherwise defaults to Inbox.

**Scope controls (toggle chips):**
- Inbox — search only the Global Inbox
- Workspace — current focused `.water` workspace only
- Workspace + Inbox — current workspace and Inbox combined
- All Workspaces — all known workspaces (+ Inbox in the full variant)

**Filter chips:**
- ★ Pinned — show only pinned notes
- ✓ Checklist — notes containing checklist blocks (heuristic)
- Image — notes containing image references (heuristic)
- Links — notes containing note or file links (heuristic)

**Result cards show:**
- Note title (bold)
- Snippet (context excerpt centered on first match, with ellipsis)
- Location breadcrumb: Inbox / Workspace › Room › Shelf or Pile
- Date last updated (YYYY-MM-DD)
- Source kind chip: "Inbox" or "WS"
- Pin indicator (★) when pinned
- Content indicators: ✓ checklist · ◻ image · ↗ links
- Open button

**Opening results:**
- Clicking "Open" or pressing Enter on the selected row saves the current note and opens the result in Editor Mode (Inbox note) or Workspace Mode (workspace note).
- If the result is in a workspace other than the currently open one, Blot opens that workspace first.

**Place button on Inbox results:**
- Inbox result cards show a "Place…" button alongside "Open".
- Clicking "Place…" opens the Place Note picker dialog for that note.
- Workspace result cards do not show "Place…" (already placed).

**Empty query:** Shows up to 30 recent notes from the current scope (ranked by recency + pin status). Does not dump all notes uncontrollably.

**Unavailable workspaces:** A warning banner appears if any known workspace file is missing or unreadable. Search continues in other sources.

**Keyboard navigation:**
- Up/down arrows move through results.
- Enter opens the selected note.
- Escape returns to Editor Mode.

**Known limitations (Prompt 7):**
- Snippet text is not visually highlighted (matched terms not bolded in the GTK label). Highlighting requires Pango markup; planned for a later prompt.
- "Selected Workspaces" scope is wired internally but not exposed in the UI as a multi-select picker yet.
- Palette tint/shape indicators are placeholder (no palette data yet).
- Bookmark indicators are not yet shown (bookmark table not populated).
- Image thumbnails are not rendered (no image fetching yet).
- FTS5 is not used yet — search runs LIKE-equivalent Rust filtering over all notes. The abstraction (`src/search/providers.rs`) is in place for a future FTS5 upgrade.

---

### Place Note (Prompt 7)

Place Note is a **move** operation: the Inbox note is archived in the Inbox and recreated in the destination workspace. It is not a copy.

**Entry points:**
- Editor toolbar: "Place Note…" button (enabled only when a note is saved)
- Desk Mode: "Place…" button on each Inbox note row
- Search Mode: "Place…" button on each Inbox result card
- Command palette: "Place Note" command (uses the currently open Inbox note)

**Picker dialog (modal):**

```
┌─────────────────────────────────────────────┐
│  Place Note                                  │
│  Note: "[note title]"                        │
│  ─────────────────────────────────────────  │
│  Workspace:  [  WorkspaceName ▾  ]           │
│  Room:       [  Room Name     ▾  ] [+ Room]  │
│                                              │
│  Destination:  ◉ Loose Notes                 │
│                ○ Shelf  [  ShelfName ▾  ] [+ Shelf] │
│                ○ Pile   [  PileName  ▾  ] [+ Pile]  │
│                                              │
│  💡 Suggested: Research › Articles (Shelf)   │
│                                              │
│  [error label — hidden when no error]        │
│  ─────────────────────────────────────────  │
│                    [Cancel]  [Place Note]    │
└─────────────────────────────────────────────┘
```

**Workspace dropdown:** Populated from the known workspace registry. Workspaces that cannot be opened (file missing or corrupt) are silently skipped. If no workspaces are registered, an info dialog is shown instead.

**Room dropdown:** Populated from the selected workspace. Changing the workspace repopulates rooms. A "+ New Room" button creates an inline prompt for a room name.

**Destination (radio buttons):**
- **Loose Notes** (default) — note is placed loose in the selected room.
- **Shelf** — a shelf combobox appears; "+ New Shelf" creates a shelf inline.
- **Pile** — a pile combobox appears; "+ New Pile" creates a pile inline.

**Suggestion:** Blot suggests a pre-selected destination based on the last room/container used in this workspace (stored in the known workspace registry). Shows a small hint label below the pickers.

**Placement transaction (safe, ordered):**
1. Validate the Inbox note exists and is not already archived.
2. Open the destination workspace.
3. Validate the destination room (and shelf/pile, if specified) exist.
4. Build a `WorkspaceNote` preserving all fields: title, body, `document_json`, `created_at`.
5. Insert the workspace note.
6. Mark the Inbox note as archived via `mark_as_placed()`.
7. Transfer global pin: if the Inbox note was pinned, unpin it and re-pin the workspace note.
8. Record the placed workspace note in the recents list.
9. Close the dialog and navigate to Workspace Mode showing the placed note.

**Partial placement warning:** If step 5 succeeds but step 6 fails (inbox archive error), Blot logs a warning and shows a non-fatal error message. The note exists in the workspace — the user should manually verify the Inbox state.

**Post-placement navigation:** After successful placement, Blot clears the editor (new blank note), opens the destination workspace, and navigates to the placed workspace note in Workspace Mode.

**Inline name creation:** The "+ New Room", "+ New Shelf", "+ New Pile" buttons open a synchronous mini-dialog within the picker. On confirm, the new item is created in the workspace and selected automatically in the dropdown.

**Keyboard:** Cancel dismisses the dialog. "Place Note" button or Enter on the focused button confirms. The dialog is modal and blocks other interactions.

---

### 4. Room Map Mode

Visual and list representation of Rooms and their Doors (connections).

**Map view:**
- Rooms are shown as labeled rectangles or cards on a canvas.
- Doors are drawn as lines connecting rooms. Line weight/style reflects connection type: normal (default), strong (bolder), weak (dashed or lighter).
- Rooms can be dragged on the canvas to arrange the map.
- Click a room to select it and see its shelves/piles in the sidebar.
- Double-click a room to navigate into it.
- Right-click a room: Rename, Add Door to..., Remove, View Notes.
- "Add Door" draws a connection to another selected room with a connection type picker.

**List/sidebar view:**
- Room list on the left, showing name, note count, shelf/pile count.
- Selected room shows its details (shelves, piles, loose notes, door connections) in the main area.
- Toggle between map view and list view from Room Map toolbar.

**Room Map actions:**
- Create Room
- Rename Room
- Add Door
- Edit Door connection type
- Remove Door
- Remove Room (must be empty or confirm migration of notes)
- Zoom and pan the canvas

---

### 5. Arrange Mode

Intentional mode for restructuring the content of a single note.

Arrange Mode is not the default editor. It is a deliberate switch into a structural editing view.

**What can be moved in Arrange Mode:**
- Paragraphs (as atomic movable units)
- Heading sections (heading + all following paragraphs until the next heading of the same or higher level)
- Checklist groups (a checklist block and its items as a unit)
- Image cards
- Callouts
- Dividers
- Embedded references (note_link, file_link, palette_reference, etc.)

**UI:**
- Blocks are shown with larger drag handles.
- Sections are visually grouped (heading + body shown as a card).
- Drag to reorder.
- Select a section or block and use Up/Down buttons in a toolbar (for keyboard users).
- "Extract to new note" button for selected content (triggers Split Note).
- "Done Arranging" button returns to Editor Mode.

Auto-bookmark fires when entering Arrange Mode if the note has any content (so the user can undo a large rearrangement).

---

### 6. Two-Panel Compare Mode

Compare two notes side by side, with the ability to copy or move content between them.

**Layout:**
```
┌───────────────────┬──────────────────────┐
│  Note A           │  Note B              │
│  [title]          │  [title]             │
│                   │                      │
│  [block content]  │  [block content]     │
│                   │                      │
│  [select blocks]  │  [select blocks]     │
└───────────────────┴──────────────────────┘
```

**Actions:**
- Select one or more blocks in Note A → "Copy to Note B" or "Move to Note B".
- Block selection highlights blocks in place.
- Move operation removes the blocks from Note A and appends/inserts them to Note B.
- Copy operation duplicates the content.
- Both operations confirm before executing.
- Auto-bookmark fires on both notes before any move.

**Opening Compare Mode:**
- Command palette → "Open Compare Mode"
- Default: current note is Note A, user picks Note B from a note picker.
- Can also be opened with two notes already selected (future).

---

## Command Palette

The command palette is a universal overlay available at all times.

**Trigger:** Keyboard shortcut (e.g., Ctrl+P or Ctrl+Shift+P) or a visible toolbar button. Trigger must be discoverable without memorizing shortcuts.

**Current commands (Prompt 7):**

| Command | Category |
|---------|----------|
| Open Desk | Desk |
| Close Desk | Desk |
| Open Focused Workspace | Desk |
| Switch Workspace | Desk |
| Pin Current Note | Desk |
| Unpin Current Note | Desk |
| Search | Navigation |
| Search All Workspaces | Navigation |
| Open Room Map | Navigation |
| New Inbox Note | Note creation |
| New Workspace Note | Note creation |
| Place Note | Note creation |
| Create Room | Workspace organization |
| Create Shelf | Workspace organization |
| Create Pile | Workspace organization |
| Convert Pile to Shelf | Workspace organization |
| Attach Palette | Note operations |
| Split Note | Note operations |
| Merge Notes | Note operations |
| Bookmark Version | Note operations |
| Show Version History | Note operations |
| Toggle Markdown Source | Note operations |
| Attach Image | Note operations |
| Open Linked File | Note operations |
| Absorb File | Note operations |
| Open Compare Mode | View modes |
| Open Arrange Mode | View modes |
| Export Note | Export |
| Export All Notes | Export |

**Palette behavior:**
- Fuzzy search through the command list.
- "Place Note" is wired (Prompt 7). It calls the Place Note picker for the currently open Inbox note. If no Inbox note is open, the command has no effect.
- Other commands log to stderr and update the status bar; full implementations arrive in later prompts.
- Recent commands appear at the top.

---

## Left Sidebar

The left sidebar is context-sensitive by mode.

| Mode | Sidebar content |
|------|-----------------|
| Editor Mode | Collapses to a slim nav rail, or shows minimal note list |
| Desk Mode | Integrated into the Desk layout |
| Search Mode | Scope and filter controls |
| Room Map | Room list with note counts |
| Arrange Mode | Block outline (list of sections/blocks for navigation) |
| Compare Mode | Hidden or minimal |

The sidebar can be hidden entirely with a toggle.

---

## Right Info Panel

Optional panel, off by default in Editor Mode.

**Contents:**
- Note metadata: created date, updated date, word count, block count
- Note placement: workspace, room, shelf/pile
- Linked objects: list of all `note_links`, `file_links`, `kindling_thread_references`, etc. in this note
- Palette attachments: palette chips with names and colors
- Bookmarks: list of named bookmarks with dates; click to restore
- Pin status and pin button

---

## Empty States

Each empty context should explain itself clearly without jargon.

| Context | Empty state message |
|---------|---------------------|
| Inbox (empty) | "Nothing in your Inbox. Notes you capture quickly appear here until you place them." |
| Room (no notes) | "This Room has no notes yet. Create a note or place one from your Inbox." |
| Shelf (no notes) | "This Shelf is empty. Add notes here or drag them from a Pile." |
| Pile (no notes) | "This Pile is empty. Loose notes land here until they're organized." |
| Search (no results) | "No notes match "[query]". Try a different term or broaden the scope." |
| Room Map (one room) | "One room so far. Add more Rooms and connect them with Doors." |

Empty states should teach the Blot model, not apologize for missing data.

---

## Visual Inheritance Display

Notes that are in Rooms with atmosphere settings show that atmosphere visually:
- Subtle background tint behind the note content area.
- Accent color on the note title or breadcrumb.
- No heavy decoration; atmosphere should feel like context, not distraction.

Notes attached to Palettes show:
- A Palette chip below the note title (shows palette name and palette color).
- Palette-colored tag indicators if the palette has tag/tint settings.

---

## Status Bar

Always visible at the bottom of the window.

| Segment | Content |
|---------|---------|
| Mode indicator | "Editor", "Desk", "Search", "Room Map", "Arrange", "Compare" |
| Location | Workspace name › Room name › Shelf/Pile name (or "Inbox" for Inbox notes) |
| Word count | "342 words" |
| Save indicator | "Saved" / "Saving…" / "Unsaved changes" |
| Terroir status | Small indicator dot: green (connected), gray (no Terroir), amber (degraded). Shown only if Terroir is configured. |

---

## Accessibility

- All interactive elements must have visible focus states.
- Color must not be the only way to convey information. Palette indicators, connection types (doors), and note states must also have text labels or icons.
- Keyboard navigation must be possible through all modes without mouse use.
- Font size should follow system preferences.
- High-contrast theme support is planned for a later phase.

---

## Open Questions

1. Should Desk Mode be a full-window overlay, a wide sidebar expansion, or a separate dedicated tab?
2. Should Room Map Mode support zoomed-out thumbnails of note content within rooms?
3. Should Arrange Mode show a mini-map of the note's block structure for long notes?
4. Should the command palette include recently visited notes as search results (not just commands)?
5. Should Compare Mode support more than two notes? (Three-panel compare is probably too complex for v1.)
6. How should note blocks render Kindling/Abacus/Fixative references when those apps are not installed?
7. Should the left sidebar in Editor Mode default to collapsed or to showing the note list for the current shelf/pile?
8. How should Blot handle very large notes (thousands of blocks) in Arrange Mode without UI sluggishness?
