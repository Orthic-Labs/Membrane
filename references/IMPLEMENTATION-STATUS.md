# Cortex implementation status

Pinned source of truth: `fe1c4cbecf7a7e20ba50d47f6b913c6fe23d19e4`.

Do not duplicate the full feature inventory here. Verify the current installation with:

```sh
cortex doctor --full --json
cortex graph manifest
cortex graph schema
cortex languages --json
cortex-watch status
```

Current stable surfaces are the `cortex`, `cortex-watch`, `cortex-mcp`, and
`cortex-install` bins declared in `package.json`. Compatibility data paths and
schema keys remain documented where the code still consumes them; no unshipped
executable alias is claimed.

Generated product and architecture documents are outputs. Edit their source
claims or generator, then regenerate; do not hand-maintain contradictory prose.
