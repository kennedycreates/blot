# AGENTS.md — Blot

Blot is the text editor and `.water` workspace editor in the Watercolor suite.

This file gives coding agents working in `blot-dev` the rules, constraints, and context they need to build correctly.

---

## Read These First

Before making any major edits or additions, read the sibling Watercolor planning docs:

- `../watercolor-dev/WATERCOLOR_BLUEPRINT.md` — suite-wide identity, principles, app list
- `../watercolor-dev/AGENTS.md` — suite-wide development rules for all apps
- `../watercolor-dev/WATER_FILE_FORMAT.md` — `.water` SQLite schema direction
- `../watercolor-dev/PALETTE_MODEL.md` — palette concepts and ownership
- `../watercolor-dev/TERROIR_CONTRACT.md` — Terroir integration rules
- `../watercolor-dev/DESIGN_SYSTEM.md` — visual language and UI copy rules
- `../watercolor-dev/APP_STRUCTURE.md` — repo layout and app independence rules
- `../watercolor-dev/ROADMAP.md` — Watercolor's build order and priorities

If you are changing how Blot stores notes, blocks, rooms, shelves, or piles, update `BLOT_DATA_MODEL.md`.
If you are changing how Blot looks or behaves, update `BLOT_UI_MODEL.md`.
If you are adding a new implementation phase, update `BLOT_ROADMAP.md`.

---

## Project Identity

Blot is a local-first Linux notes and `.water` workspace editor.

It is one of the hearts of the Watercolor suite. It is the main surface for writing, note-taking, thought capture, and scratchpad use.

Blot opens `.water` files and gives them a writing-focused interface organized around Rooms, Shelves, Piles, and Loose Notes.

Blot also maintains a private app-level Global Inbox for quick capture outside of any workspace.

## Current Watercolor/Terroir v0.1 Alignment

Terroir v0.1 currently defines `.water` files as inspectable JSON with:

- `format_name = "watercolor.workspace"`
- `format_version = "0.1"`
- workspace, palette, object, relationship arrays
- stable ID prefixes: `wcw_`, `wcp_`, `wco_`, `wcr_`

Some older Blot planning docs describe future SQLite-backed `.water` files and Blot-specific tables. Treat those as forward-looking until the suite-wide `.water` schema is reconciled with Terroir v0.1. For current work:

- `.water` files are the source of truth.
- Blot must open `.water` files directly even if Terroir is unavailable.
- Blot should use Terroir only as optional indexing/context infrastructure.
- Blot must not depend on Terroir for basic editing.
- Blot must not make `.water` changes through Terroir.

---

## What Blot Is Not

Do not turn Blot into any of these:

- **Obsidian** — no Markdown vault, no graph mind map as a primary mode, no plugin ecosystem, no `.md` files as source of truth
- **Notion** — no wiki, no database tables, no team pages, no nested page trees
- **LibreOffice / Google Docs** — no full office suite, no pagination, no mail merge, no complex document formatting
- **Roam Research** — no backlinks as a primary navigation concept
- **Bear / Typora** — not a Markdown-native editor with `.md` as the source format
- **Logseq** — not an outliner-first system
- **Joplin** — not a cloud sync notebook manager
- **A cloud collaboration app** — Blot has no real-time collaboration, no sharing, no server

Blot is a personal, local-first writing and workspace tool. It connects to the Watercolor suite through `.water` files. It is not a generic document editor.

---

## Non-Negotiable Product Rules

These rules are fixed. Do not change them without explicit user instruction.

1. **Open to writing immediately.** Blot must open into a blank note. No workspace picker, no title dialog, no template chooser, no folder selection. The user writes first.

2. **Any non-empty note must be saved.** Truly blank notes may be discarded silently. All others must be autosaved.

3. **Auto-title new notes.** Derive the title from the first heading, first meaningful line, or a timestamp fallback. Never demand a title up front.

4. **The Global Inbox is outside `.water` files.** Inbox notes are private to Blot. They are invisible to Lattice, Kindling, Abacus, Fixative, and Terroir until the user explicitly places a note into a workspace.

5. **A note lives in exactly one location at a time.** A note may be in the Inbox, Loose in a workspace, on a Shelf, or in a Pile — never in two places simultaneously.

6. **Notes use a structured block model internally.** Raw Markdown is not the source of truth. The user-facing experience may feel Markdown-ish and a raw source toggle must exist, but internal storage uses blocks in the `.water` SQLite format.

7. **Every workspace has at least one Room.** The first Room is renamable and not permanently special.

8. **Shelves are intentional; Piles are loose.** A Pile can be converted into a Shelf. They are not the same concept.

9. **Rooms connect through Doors.** Doors have a connection type (normal, strong, weak) and are unlabeled by default. Room Map mode must support both visual and list/sidebar views.

10. **Visual inheritance is layered.** Notes inherit atmosphere from their Room. Notes inherit tags, tints, and shapes from Palette. Room controls atmosphere. Palette controls meaning.

11. **Mouse-first, genuinely keyboard-capable.** Every major action must have a visible mouse path and a command palette entry. Memorized shortcuts are optional, not required.

12. **Command palette is required from day one.** See `BLOT_UI_MODEL.md` for the required initial command list.

13. **Tabs and multiple windows are required.**

14. **Auto-bookmark before risky operations.** Split, Merge, Absorb, and large Arrange edits must create an automatic bookmark before executing.

15. **Blot must work without Terroir.** Terroir improves search and cross-workspace context. If Terroir is unavailable, Blot must still open `.water` files, read notes, write notes, and let the user work.

---

## Architecture Constraints

- **Language:** Rust
- **UI toolkit:** GTK4 via gtk-rs
- **Storage:** Inbox is a standalone SQLite file at `~/.local/share/blot/inbox.db`. For v0.1, `.water` workspace files are JSON per Terroir's canonical format doc. Older SQLite-backed workspace notes below are forward-looking until the suite format is reconciled.
- **XDG compliance:** Config in `~/.config/blot/`, cache in `~/.cache/blot/`, state in `~/.local/share/blot/`.
- **No cloud dependency.**
- **No network calls during normal operation.**
- **No collaboration or sync features.**
- **Integration with other Watercolor apps happens through documented contracts** (`.water` file schema, Terroir APIs, desktop files, XDG paths). Do not import other Watercolor app internals.

### Future `.water` File Extensions for Blot

Do not implement these against the current JSON v0.1 format. If the suite later moves `.water` to SQLite, Blot-specific tables should use the `blot_` prefix to avoid collisions with other Watercolor apps:

- `blot_rooms`
- `blot_room_connections`
- `blot_shelves` (stores both shelves and piles; distinguished by `kind` column)
- `note_blocks`
- `note_placements`
- `note_bookmarks`
- `note_links`
- `note_object_links`
- `note_pins`
- `blot_search_index` (FTS5)

The shared `notes` table from `WATER_FILE_FORMAT.md` is used for base note metadata. Blot must not break the shared note schema.

See `BLOT_DATA_MODEL.md` for the full entity list.

---

## UX Principles

- Writing comes first. Every UI decision should protect the calm writing surface.
- Fast capture is more important than perfect organization at creation time.
- The Inbox is a quarantine zone, not a permanent home. Blot should gently encourage placement without shaming the user.
- Organization happens after capture, not before.
- Search is a first-class mode, not a buried feature.
- Visible UI surfaces are primary. Keyboard shortcuts are supplementary.
- Bookmarks and version history are safety nets, not daily workflow. Keep them tucked away.
- Splitting, merging, and absorbing notes are intentional power operations. Confirm before destructive steps.
- Export must always work. Users must never feel trapped in Blot's format.

---

## Commands

Blot does not have source code yet. These will be filled in when the Rust project is initialized.

Expected commands once implemented:

```sh
cargo build
cargo build --release
cargo test
cargo clippy
cargo fmt
```

Expected launch command (development):

```sh
cargo run
```

---

## Documentation Style

Follow the style in `../watercolor-dev/AGENTS.md`:

- Concise but precise.
- Use "what this is not" sections when useful.
- Prefer concrete examples over inspiration language.
- Plain action labels in UI copy. Never use "Summon," "Conjure," "Dispatch to the Atelier," or similar.
- Be honest about open questions and tradeoffs.

---

## Agent Behavior in This Repo

1. Read the sibling Watercolor docs listed at the top of this file before major changes.
2. Do not add a feature that turns Blot into Obsidian, Notion, LibreOffice, or a Markdown vault.
3. Do not add cloud sync, collaboration, sharing, or network features.
4. Do not break the Blot-Terroir independence guarantee: Blot must work without Terroir running.
5. Do not break the shared `.water` notes schema. Blot-specific additions must be additive.
6. Update the relevant planning doc when the model, UI, or roadmap changes.
7. Ask before making major conceptual changes to note organization, block types, or room structure.
8. Keep the Inbox strictly separate from `.water` workspace data.
9. Auto-bookmark before any operation that overwrites note content non-reversibly.
10. Do not invent integration seams with other Watercolor apps beyond what is documented in the Terroir contract and `.water` schema.
