# Compatibility

Tracked ground truth is `release/compatibility.template.json`. Final
artifact-bound `compatibility.json` is generated from sealed bytes under
ignored `.right-release/` output and published as a release asset.

## Store schema and migration

- Current store schema version: **17**.
- Minimum importable schema version: **12** (`minImportableSchemaVersion`).
  Stores older than v12 cannot be opened directly.
- The database is backed up before any incompatible migration
  (`backupBeforeMigration: true`).
- A torn migration is recoverable via
  `repairInterruptedMigration(dbPath, fromVersion)`.

## Platform matrix

| Platform | Arch | Filesystem notes |
|---|---|---|
| macOS | arm64, x64 | Case-insensitive by default |
| Linux | x64, arm64 | Case-sensitive |
| Windows | x64, arm64 | Case-insensitive; UNC and junction paths supported |

## Language depth tiers

`languageDepth` groups supported languages into tier A/B/C by grammar and
fixture depth (tier A is the deepest). As documented in `tierNote` on that
field: **tiers reflect grammar and fixture depth, not compiler-backed
verification** — compiler-backed exact references currently cover only
JS/TS (native compiler adapter) plus imported SCIP indexes for other
languages. A language appearing in tier A is not a claim of compiler-level
precision; it is a claim of grammar/fixture coverage.

## Security and threat model

- Security policy: `.github/SECURITY.md`.
- Threat model and control-to-gate mapping: `docs/reference/threat-model.md`.
- Generated `compatibility.json` security block links both, plus the
  qualification gate (`qualification.yml`) that enforces them on every
  release.

## Related

- `docs/operations/support.md` — supported version lines.
- `docs/operations/uninstall.md` — removal paths per platform.
