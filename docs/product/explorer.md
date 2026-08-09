# Local evidence explorer

`cortex explore` starts a loopback-only, read-only HTTP server with a static
UI. It binds to `127.0.0.1`, uses an unguessable session token (held in
memory only, never logged), and shuts down with the CLI unless installed as a
service.

## Views

- Architecture / layers
- Impact
- Routes / services / queues
- Document claims / conflicts
- Evidence inspector
- Freshness / provider coverage
- Rule violations / drift
- Provenance timeline
- Search
- "Why this result"

Every visual item opens source evidence and displays provider, precision,
confidence, generation, freshness, omissions, and truncation.

## Security

- Loopback only; no network-capable API.
- Browser code uses only application-service endpoints — no SQLite handles,
  no repository files, no host credentials.
- No giant graph is the default UX.

## Exports

JSON, Markdown evidence packs, Mermaid, SARIF, and on-demand full JSON. SQLite
remains the sole persisted graph; exports never create a second truth store.
