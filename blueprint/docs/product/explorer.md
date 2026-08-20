# Local evidence explorer

Status: implemented and tested; not yet published.

`blueprint explore` starts a loopback-only, read-only HTTP server with a static
UI. It binds to `127.0.0.1`, uses an unguessable session token (held in
memory only, never logged), and shuts down with the CLI. Standalone CLI prints
the authenticated URL for the user to open; it never exposes the token through
browser child-process arguments. The tray opens the same UI inside its Tauri
webview with the token held in process memory.

## Views

- Architecture / layers
- Evidence inspector
- Freshness status
- Search
- Optional 3D orb layout

Every visual item opens its returned service evidence. Layout is deterministic,
keyboard focus remains visible, and reduced-motion settings are honored.

## Security

- Loopback only; no network-capable API.
- Browser code uses only application-service endpoints — no SQLite handles,
  no repository files, no host credentials.
- No giant graph is the default UX.

SQLite remains the sole persisted graph; Explorer creates no second truth store.
