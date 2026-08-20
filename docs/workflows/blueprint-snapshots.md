# Blueprint snapshot review flow

An operator creates a named immutable snapshot with Blueprint CLI after a clean, complete build:

```sh
blueprint graph snapshot create --name base --json
```

Agents use read-only `membrane_blueprint` operations against Blueprint commit `96dc3e6`:

- `snapshot_get` with `name` reads one identity and exact file leaves.
- `snapshot_list` lists named identities.
- `changes_since` with `name` and bounded `limit` returns ordered changes plus a truncation receipt.

Membrane invokes fixed `node blueprint/scripts/blueprint.mjs graph ... --json` argv, never a shell. It accepts only generation IDs, manifest/content hashes, clean Git identities, safe paths, change kinds, and bounded receipts. Missing, dirty, stale, malformed, mismatched, or non-JSON output becomes `blueprint_unavailable`; CLI stderr is not returned.
