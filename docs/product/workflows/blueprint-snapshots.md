# Blueprint snapshot review flow

An operator prepares a complete native Blueprint generation from a clean worktree:

```sh
membrane blueprint build --repo-root . --deadline-ms 120000
```

Native Rust Blueprint owns named snapshot creation & read-only review. Agents use
`membrane_blueprint` operations through installed Membrane with Hub on or off:

- `snapshot_get` with `name` reads one identity and exact file leaves.
- `snapshot_list` lists named identities.
- `changes_since` with `name` and bounded `limit` returns ordered changes plus a truncation receipt.

While tray-owned daemon is active, Membrane uses Blueprint's typed named-pipe service; Blueprint owns watcher semantics & daemon hosts its resident execution. An unenrolled repository is `not_configured`, stale or incomplete evidence is `degraded`, and only a transport/service failure is `blueprint_unavailable`. With tray inactive, explicit Membrane operations remain available; Blueprint requests use bounded one-shot execution, start no watcher & exit after completion. Generation/hash mismatches remain typed fail-closed omissions.
