# Blot Journal

## 2026-05-21 — `.water` v0.1 Parser Alignment

### Format Decisions

Blot now mirrors Terroir's canonical JSON `.water` v0.1 format for direct loading. The agreed contract is:

- `format_name = "watercolor.workspace"`
- `format_version = "0.1"`
- required top-level fields: `workspace_id`, `workspace_name`, `palettes`, `objects`, `relationships`
- ID prefixes: `wcw_`, `wcp_`, `wco_`, `wcr_`
- supported object types: `note`, `asset`, `task`
- supported target kinds: `object`, `path`
- supported relationship types: `references`, `explained_by`, `implemented_in`

Terroir remains the canonical doc source at `../terroir-dev/docs/WATER_FILE_FORMAT.md`; Blot has `docs/WATER_FILE_FORMAT.md` as a local reference.

### Validation Behavior

Added `src/water_file.rs` with direct parse/validation support. Blot validates malformed JSON, unsupported versions, required field emptiness, unique IDs, palette object references, relationship source objects, object relationship targets, supported vocabularies, and non-empty path strings for `file_refs[]` / path relationships.

Missing files referenced by `file_refs[]` or `target_path` are allowed by format validation. They should be reported later as diagnostics/context, not block parsing.

### Fixtures Added

Copied Terroir's shared examples into `tests/fixtures/`:

- `basic.water`
- `multiple_palettes.water`
- `broken_refs.water`
- `unsupported_version.water`
- `malformed.water`

Blot tests now parse the valid fixtures and reject broken, malformed, unsupported, and unsupported-vocabulary cases.

### Known Limitations

- The parser is duplicated from Terroir to keep repos independent for now. A future shared `water-file` crate would reduce drift.
- Blot still does not wire `.water` launch paths into a workspace UI.
- Blot does not write `.water` files yet.
- Older Blot planning docs still describe a future SQLite-backed `.water` direction; current v0.1 implementation follows Terroir's JSON contract.

### Commands Run

- `cargo fmt` — passed
- `cargo check` — passed
- `cargo test` — passed, 45 tests

### Next Step

Wire Blot's direct `.water` parser into launch handling with a read-only workspace summary view. Do not add rich editing, repair, sync, cloud, collaboration, or Terroir-required behavior.

## 2026-05-21 — Watercolor/Terroir v0.1 Alignment Inspection

### Current App State

Blot is currently a Rust 2021 GTK4 app at Prompt 2/12. It launches into a writing surface backed by a private Global Inbox SQLite database and autosaves non-blank notes after a 1.5 second debounce. Desk, Search, Room Map, and command palette shells exist; Search and Room Map are placeholders. `.water` workspace launch arguments are parsed but workspace editing is still a stub.

### Current Architecture and Data Model

- Entry point: `src/main.rs` creates a GTK application and parses custom launch flags before handing activation to `src/app.rs`.
- App setup: `src/app.rs` resolves XDG paths, loads config/theme CSS, opens the Global Inbox database, installs dev desktop assets in debug builds, and presents `MainWindow`.
- UI: `src/ui/main_window.rs` owns the main GTK window, mode stack, status bar, editor, Desk shell, placeholder Search/Room Map shells, command palette action, and save-on-close behavior.
- Inbox data: `src/inbox.rs` stores private Blot notes in `~/.local/share/blot/inbox.db` using `inbox_notes`, `inbox_note_revisions`, and `inbox_schema_version`.
- Note text: the current editor stores flat plain text, not structured blocks yet.
- Title logic: `src/title.rs` derives titles from Markdown-ish headings or first meaningful line and handles blank/word-count helpers.
- Config/paths: `src/config.rs` and `src/paths.rs` provide a small TOML config parser and XDG path helpers.

### Standards Added

- `AGENTS.md` now includes a current Watercolor/Terroir v0.1 alignment section.
- `README.md` now states that Blot must open/edit `.water` files directly and must treat Terroir as optional context/indexing infrastructure.
- Added `docs/BLOT_V0.1_ALIGNMENT.md` to capture the current implementation, data model, `.water` support gap, risks, and next implementation step.

### Important Alignment Finding

There is a schema-direction mismatch to resolve before implementing `.water` editing: Terroir v0.1 currently documents JSON `.water` files, while older Blot planning docs describe future SQLite-backed `.water` workspace files and Blot-specific tables. Blot should follow the current Terroir contract for v0.1 and avoid implementing the older SQLite `.water` assumptions until the suite-wide format is reconciled.

### Commands Run

- `cargo fmt` — passed
- `cargo check` — passed
- `cargo test` — passed

### Next Step

Implement a minimal read-only `.water` v0.1 JSON parser/open path in Blot that does not require Terroir, then add an optional Terroir client only for context refresh/indexing once direct open works.
