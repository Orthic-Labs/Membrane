# Cortex implementation status

Pinned source of truth: this file ships with the matching Cortex package and release commit.

`sol.md` owns product/performance scope. `solimplement.md` owns current implementation sequencing & delivery gates. Executable evidence below owns live status.

Do not duplicate the full feature inventory here. Verify the current installation with:

```sh
cortex doctor --full --json
cortex graph manifest
cortex graph schema
cortex languages --json
cortex-watch status
```

Current stable surfaces are the `cortex`, `cortex-watch`, `cortex-mcp`, and
`cortex-install` bins declared in `package.json`. `cortex explore` serves the
authenticated, loopback-only interactive Explorer used by the desktop tray.
Explorer reads the canonical SQLite graph and persists no second truth store.
Compatibility data paths and schema keys remain documented where the code still
consumes them; no unshipped executable alias is claimed.

Generated product and architecture documents are outputs. Edit their source
claims or generator, then regenerate; do not hand-maintain contradictory prose.
