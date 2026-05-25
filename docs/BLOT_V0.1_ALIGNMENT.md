# Blot v0.1 Alignment

This document records Blot's current implementation state and how it should align with Watercolor/Terroir v0.1.

## Current App State

Blot is a local-first GTK4 notes app with a private Global Inbox and a basic autosave editor.

Implemented:

- GTK4 application shell.
- XDG path helpers for config, data, cache, themes, and Inbox database.
- Generated config file with theme selection.
- Private Inbox SQLite database at `~/.local/share/blot/inbox.db`.
- Autosave editor for flat text Inbox notes.
- Smart title derivation from heading or first meaningful line.
- Desk shell listing Inbox notes.
- Command palette shell.
- Search and Room Map placeholder shells.
- Launch parsing for `.water` workspace paths and modes.
- Direct JSON `.water` launch/open support for v0.1 files.
- Basic direct `.water` note object listing and plain text body editing.
- Safe JSON `.water` saving through validation, temporary file write, and timestamped backup.

Not implemented yet:

- Structured block document model.
- Workspace Rooms, Shelves, Piles, Doors, or note placement.
- Terroir client.
- Search index.

## Current File/Data Model

Blot currently has one real data store: the private Inbox database.

Inbox path:

```text
~/.local/share/blot/inbox.db
```

Tables currently created:

- `inbox_notes`
- `inbox_note_revisions`
- `inbox_schema_version`

Current editor content is stored as flat plain text in `inbox_notes.body`. This is a Prompt 2 implementation detail. The planned block model is documented in `BLOT_DATA_MODEL.md` but is not implemented yet.

Launch flags already accept `.water` paths:

```text
blot <path.water>
blot --workspace <path.water>
blot --new-workspace-note <path.water>
```

Those flags now open JSON `.water` v0.1 files directly. Blot shows the workspace name, lists note objects, lets the user edit note title/body, and saves changes back to the `.water` file. `--new-workspace-note` remains limited to the older SQLite-backed workspace path until JSON note creation is specified.

## Watercolor/Terroir v0.1 Contract

For current Watercolor/Terroir v0.1 alignment:

- `.water` files are the source of truth.
- Blot must open `.water` files directly even if Terroir is unavailable.
- Blot should use Terroir when available for indexing/context refresh.
- Blot must not depend on Terroir for basic editing.
- Blot must not ask Terroir to mutate `.water` files.
- The Inbox remains private to Blot until the user explicitly places a note into a workspace.

Terroir v0.1 currently documents `.water` as JSON with:

- `format_name = "watercolor.workspace"`
- `format_version = "0.1"`
- `workspace_id`
- `workspace_name`
- `palettes[]`
- `objects[]`
- `relationships[]`

Required ID prefixes:

- `wcw_` workspace IDs
- `wcp_` palette IDs
- `wco_` object IDs
- `wcr_` relationship IDs

Terroir socket API is optional and available at:

```text
$XDG_RUNTIME_DIR/watercolor/terroir/terroir.sock
```

Useful Terroir methods for future Blot integration:

- `status`
- `list_workspaces`
- `context_for_path`
- `doctor_summary`
- `reindex`

Blot should use short socket timeouts and treat unavailable Terroir as normal.

## Changes Needed for `.water` Support

1. Reconcile the older SQLite-backed workspace shell with the JSON `.water` v0.1 direct-edit shell.
2. Specify JSON note creation and placement before enabling new note creation in JSON `.water` files.
3. Add explicit UI open/save error states beyond the current dialog/status-label reporting.
4. Only after direct open/save works, add an optional Terroir client for `status` and `reindex`.
5. Reconcile old SQLite-backed Blot planning docs with Terroir's JSON `.water` v0.1 before expanding write support.

## Risks

- Existing Blot planning docs describe SQLite-backed `.water` files, but Terroir v0.1 documents JSON `.water` files. Blot now has both an older SQLite workspace shell and a direct JSON `.water` shell; these need reconciliation.
- Current Inbox database path in code/README is `inbox.db`; older docs mention `inbox.sqlite`.
- Current editor stores flat text, so moving Inbox notes into `.water` workspaces should wait until the block model and `.water` schema are stable.
- Terroir `context_for_path` matches exact indexed path text, not canonical equivalents.
- Terroir has no socket API version negotiation yet.

## Next Implementation Step

Build out the direct `.water` v0.1 JSON editing path:

- Add JSON note creation only after the object placement rules are specified.
- Keep palette editing visual-only work out of scope until the direct note editor is stable.
- Do not add sync, cloud, collaboration, AI, graph, formula, task, or project-management behavior.
