# Touchline FM automation

For supported FM reads and edits, use the persistent Touchline API first. Do not
create another raw-memory script for a feature already exposed by the editor.

- Client: `tools/touchline_api.py` (Python standard library only).
- `start` reuses or starts the hidden local server; `connect` builds the index.
  Check `status` and reuse a live connection instead of reconnecting each call.
- `capabilities` lists current commands and argument shapes. Read `detail` or
  `field_catalog` for actual field keys and limits; do not guess them.
- Scalar changes: detail → preview → inspect → apply. Batch changes use explicit
  targets, maximum 200 per preview. Relation and event commands have their own
  preview/apply pairs. User-authorized edits do not need another confirmation.
- Never automatically retry a write after a transport error. Inspect its history
  and current detail first. No raw addresses or bindings belong in API requests.
- City `group` is the actual nation name. All cities are indexed, including those
  without clubs. Attraction is 0–20; addition does not automatically clamp. Query
  current values and separate capped targets before previewing a bulk increase.
- Save/load invalidates cached identities and preview tokens. Use
  `connection_poll` / `connection_reconnect`, then read and preview again.
- The API does not advance, load, or save the game. Do not modify FM just to test
  the API; use offline fixtures and read-only live queries/previews.
- API source: `src/api.rs`; semantic dispatch: `src/bridge/api.rs`.
  Build: `build-api.ps1`. Contract: `reference/local-api.md`.
