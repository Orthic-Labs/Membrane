# Blueprint SQLite graph store

The persistent layer for a Blueprint repository. One `graph.db` per enrolled repo,
under `.agent/graph/`. Everything the CLI, MCP, SDK, hooks, and the resident
service read goes through `graph/store-sqlite.mjs` — there is no parallel
"cache DB" and no graph.json.

## Schema versioning and N-2 migration

- `SCHEMA_VERSION` is derived from the migration list, never hardcoded.
- `migrate()` upgrades an older store in place, one transactional migration at a
  time. A crash between migrations leaves the store at the last committed
  version; re-running migrate() resumes from there.
- Every non-empty store that is about to be upgraded is **backed up first** to
  `<dir>/backups/graph.db.before-migrate-v<N>`. The WAL is checkpointed before
  the copy so the backup reflects every committed write.
- **N-2 support:** stores from the previous two minor schema lines migrate
  directly to the current line. The fixture builder in
  `fixtures/stores/build-stores.mjs` materialises real schema-v13 and v12
  databases, and `tests/store-migrations.test.mjs` proves both upgrade without
  losing rows.

### Repairing an interrupted migration

`repairInterruptedMigration(dbPath, fromVersion)` restores the pre-migration
backup and re-runs migrate(). It returns `{ restored: true, fromVersion }` or a
typed `{ restored: false, reason }` when no backup exists — it never throws
into a repair plan. `blueprint doctor` surfaces a missing repair path as a finding.

## Platform path compatibility

- Repository identity and root confinement live in
  `lib/application/root-registry.mjs`; roots are `realpath`'d so symlinked
  checkouts, trailing-separator variants, and case-only spellings on
  case-insensitive filesystems collapse to one canonical root.
- Store files are always opened **under** the repo root; path normalisation
  replaces backslashes with forward slashes on every read surface so Windows
  UNC/junction paths cannot change repository identity or escape scope.
- `tests/cross-platform-paths.test.mjs` runs the same assertions on
  case-sensitive CI and case-insensitive developer machines.

## Performance envelopes

Budgets per repository class live in `evals/performance-envelopes.json`
and are enforced by `tests/performance-envelopes.test.mjs` in CI with a 4x
slack multiplier. A waiver requires a machine-readable rationale and a future
expiry. See `docs/benchmarks/performance-envelopes.md`.
