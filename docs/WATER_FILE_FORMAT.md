# `.water` Workspace Format v0.1

The canonical `.water` v0.1 format document currently lives in Terroir:

```text
../terroir-dev/docs/WATER_FILE_FORMAT.md
```

Blot mirrors that contract for direct file loading so it can open `.water` files without requiring Terroir.

Current v0.1 decisions:

- `.water` files are JSON and human-inspectable.
- `format_name` is `watercolor.workspace`.
- `format_version` is `0.1`.
- Required top-level fields are `workspace_id`, `workspace_name`, `palettes`, `objects`, and `relationships`.
- Required ID prefixes are `wcw_`, `wcp_`, `wco_`, and `wcr_`.
- Supported object types are `note`, `asset`, and `task`.
- Note objects may include an optional `body` string for Blot-editable plain text content.
- Blot also reads an optional `content` string on note objects for compatibility, but writes edits to `body`.
- Supported relationship target kinds are `object` and `path`.
- Supported relationship types are `references`, `explained_by`, and `implemented_in`.
- Palette `object_ids` must point to objects in the same workspace.
- Relationship `source_object_id` must point to an object in the same workspace.
- For `target_kind = "object"`, `target_object_id` is required and must exist in the same workspace.
- For `target_kind = "path"`, `target_path` is required and must be a non-empty string.
- `file_refs[]` and `target_path` may point to missing files. Missing files are diagnostics, not parse failures.

Blot currently duplicates the small parser to keep repos independent. A future shared `water-file` crate would reduce drift once the format stabilizes.

## Blot Direct-Edit Extension

For v0.1 direct editing, Blot adds the smallest compatible note content field:

```json
{
  "object_id": "wco_note",
  "object_type": "note",
  "title": "Project Brief",
  "summary": "A short planning note",
  "app_origin": "blot",
  "file_refs": [],
  "body": "Editable note text"
}
```

Rules:

- `body` is optional and only meaningful for `object_type = "note"`.
- If a note has `content` but no `body`, Blot displays `content`.
- When the user edits note text, Blot writes the new text to `body`.
- Unknown valid fields are preserved when Blot loads and saves a file.
- Blot validates the workspace before saving and does not write malformed or unsupported `.water` files.
- Saves write a temporary file first, create a timestamped `.bak` copy of the previous file, then replace the original.
