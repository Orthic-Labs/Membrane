# Blueprint snapshot review flow

An operator creates a named immutable snapshot with Blueprint CLI after a clean, complete build:

```sh
blueprint graph snapshot create --name base --json
```

Agents use read-only `membrane_blueprint` operations through daemon-hosted Blueprint:

- `snapshot_get` with `name` reads one identity and exact file leaves.
- `snapshot_list` lists named identities.
- `changes_since` with `name` and bounded `limit` returns ordered changes plus a truncation receipt.

While tray-owned daemon is active, Membrane uses Blueprint's typed named-pipe service; Blueprint owns watcher semantics & daemon hosts its resident execution. An unenrolled repository is `not_configured`, stale or incomplete evidence is `degraded`, and only a transport/service failure is `blueprint_unavailable`. With tray inactive, Membrane remains unavailable, while an explicit direct Blueprint request routes to its bounded one-shot client, never starts a watcher, and exits. Generation/hash mismatches remain typed fail-closed omissions.
