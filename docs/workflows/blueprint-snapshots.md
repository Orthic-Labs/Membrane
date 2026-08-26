# Blueprint snapshot review flow

An operator creates a named immutable snapshot with Blueprint CLI after a clean, complete build:

```sh
blueprint graph snapshot create --name base --json
```

Agents use read-only `membrane_blueprint` operations through Hub-hosted Blueprint:

- `snapshot_get` with `name` reads one identity and exact file leaves.
- `snapshot_list` lists named identities.
- `changes_since` with `name` and bounded `limit` returns ordered changes plus a truncation receipt.

While Hub is active, Membrane uses Blueprint's typed named-pipe service; Hub owns watcher lifecycle. An unenrolled repository is `not_configured`, stale or incomplete evidence is `degraded`, and only a transport/service failure is `blueprint_unavailable`. With Hub inactive, Membrane remains unavailable, while an explicit direct Blueprint request routes to its bounded one-shot client, never starts a watcher, and exits. Generation/hash mismatches remain typed fail-closed omissions.
