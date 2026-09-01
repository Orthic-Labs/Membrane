# Blueprint implementation status

Pinned source of truth: this file ships with the matching Blueprint package and release commit.

Executable evidence and the code own live status.

Do not duplicate the full feature inventory here. Verify the current installation with:

```sh
blueprint doctor --full --json
blueprint graph manifest
blueprint graph schema
blueprint languages --json
blueprint-watch status
```

Current stable surfaces are the `blueprint`, `blueprint-watch`, `blueprint-mcp`, and
`blueprint-install` bins declared in `package.json`. `blueprint explore` serves the
authenticated, loopback-only interactive Explorer used by the desktop tray.
Explorer reads the canonical SQLite graph and persists no second truth store.
Compatibility data paths and schema keys remain documented where the code still
consumes them; no unshipped executable alias is claimed.

Generated product and architecture documents are outputs. Edit their source
claims or generator, then regenerate; do not hand-maintain contradictory prose.
