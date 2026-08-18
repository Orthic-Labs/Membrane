# Membrane Privacy & Provenance

> MBR-502 — what the runtime records, what it never records, where it is
> stored, who owns retention, and how a user can audit or wipe the trail.

## 1. Scope

This document is the canonical, user-facing privacy contract for the
Membrane runtime's provenance adapter. It complements the technical
contract in `engine/crates/membrane-runtime/src/provenance.rs` and the
TypeScript twin in `mcp/adapters/provenance/index.mjs`. If the runtime
behaviour and this document disagree, the runtime is the source of truth
and this document must be updated in the same change.

The adapter is **passive**: it captures metadata about a host-driven
read, never the read payload itself. It is wired in front of every
observable read path that lands in the data plane (HTTP service,
CLI/migration verbs, install/uninstall).

## 2. What is recorded

For every host-driven read, the adapter appends one JSONL row to
`<MEMBRANE_DATA_ROOT>/provenance.jsonl`. The row contains:

| Field                | Source                                            |
|----------------------|---------------------------------------------------|
| `schemaVersion`      | Bumped on any breaking change to the row shape.   |
| `installationId`     | Stable per-installation id from `installation_identity`. |
| `operation`          | Short verb (`read`, `observe`, `install`, `uninstall`, …). |
| `clientId`           | Client identifier when known (`claude`, `codex`, …). |
| `scopeGrant`         | Scope-grant token or digest that authorised the read. |
| `workingTree`        | Captured via three read-only `git` subcommands.   |
| `recordedAtUnixMs`   | Wall-clock time the row was appended.             |

The `workingTree` block is itself a small, structured object:

- `gitHead` — short/full SHA from `git rev-parse --verify HEAD`. `null`
  when the workspace has no commit yet (a fresh install).
- `dirtyPaths` — paths reported by `git status --porcelain`. These are
  project-relative; git never returns absolute paths here.
- `diffAdded` / `diffRemoved` — sums of the `git diff --numstat`
  columns. Binary files report `-` `-` and map to zero.
- `capturedAtUnixMs` — wall-clock time the snapshot was captured.

No field listed above identifies a user by name, email, IP address, or
device fingerprint. The `installationId` is a per-installation opaque
token that is already documented in `installation_identity`.

## 3. What is NEVER recorded

The adapter is intentionally narrow. The following are explicitly
excluded from every row:

- **File bodies.** The adapter only shells out to three git subcommands
  (`rev-parse --verify HEAD`, `status --porcelain`, `diff --numstat`).
  It never opens a file, never reads a blob, never reads a chunk.
- **Paths outside the workspace.** `git` returns project-relative paths
  by convention; the adapter forwards them verbatim and never resolves
  them to absolute paths. Snapshots are scoped to the workspace root
  the caller passes.
- **Network payloads.** The adapter opens no TCP/TLS socket. The only
  external process it invokes is `git`, and only against the workspace
  directory. The runtime separately records network-level events
  through `record_observable_event` (MBR-501), which has its own
  privacy contract.
- **Environment variables.** `GIT_TERMINAL_PROMPT=0` and
  `GIT_OPTIONAL_LOCKS=0` are set on the git invocation to prevent
  hangs, but the adapter does not read, log, or persist any
  environment variable.
- **Credentials, secrets, or tokens.** None of the recorded fields
  carry secret material. The `scopeGrant` field carries a token or
  digest **only** when the caller already passed one through (the
  adapter does not synthesise one).

## 4. Storage

The journal lives at:

```
<MEMBRANE_DATA_ROOT>/provenance.jsonl
```

`<MEMBRANE_DATA_ROOT>` is resolved by the runtime's `paths::data_root`
helper. On macOS that is `~/Library/Application Support/Membrane`; on
Linux it is `$XDG_DATA_HOME/membrane`; on Windows it is
`%LOCALAPPDATA%\Membrane`. The `MEMBRANE_DATA_ROOT` environment
variable overrides the resolution for tests and operators.

The file is **JSONL** (one JSON object per line, newline-terminated).
There is no header and no trailer; every line is a self-contained
`ProvenanceRowV1`. The schema is baked into the runtime constant
`PROVENANCE_ROW_SCHEMA_VERSION = 1` and the parallel
`WORKING_TREE_SNAPSHOT_SCHEMA_VERSION = 1` so a future migration can
detect and re-stamp old rows without rewriting the file.

The adapter is **append-only**. It does not rotate, truncate, or delete
the file. It does not lock the file; concurrent writers from multiple
processes may interleave lines, but each line is independently valid.

## 5. Retention

The adapter does not enforce retention. The runtime's existing
retention policy governs the journal. The default policy is:

- Keep the journal for the lifetime of the installation.
- A wipe is initiated **only** by the user, via the CLI uninstall
  verb (see §6). The uninstall receipt records the wipe path so a
  forensic reviewer can prove the data was removed.
- If the user wants a shorter retention window, they can pre-delete
  the journal; the adapter will simply create a new file on the next
  append.

Operators that need a hard retention cap should add an external
cron-driven truncation job that rotates the file by atomic rename and
keeps one or more gzip-compressed snapshots. The adapter does not
implement rotation because rotation policy varies by deployment.

## 6. User rights

A user can audit or wipe the journal from the existing CLI surface.

### Audit (read-only)

```sh
# Print the journal to stdout, line-by-line.
cat "$MEMBRANE_DATA_ROOT/provenance.jsonl"

# Pipe through jq for human-readable inspection.
cat "$MEMBRANE_DATA_ROOT/provenance.jsonl" | jq .
```

The runtime's `doctor` and `observe` subcommands also surface the
journal's path and size so the user can confirm the file exists
without opening a shell.

### Wipe

The journal is **owned by the runtime** and is removed by the
uninstall verb:

```sh
# Removes the data root entirely, including provenance.jsonl.
crypt uninstall
# or, in the workspace:
tools/bin/crypt uninstall
```

The install verb (`crypt install`) does **not** delete the journal; an
install preserves the trail across upgrades. The `observe` verb emits
a row but does not act on the journal.

## 7. Threat model

The adapter records metadata only. The worst-case disclosure is a
cleartext list of workspace-relative paths that were dirty at the
moment of a read, plus the diff size in lines. None of the recorded
fields carry payload bytes, keys, or user identifiers. The journal
file inherits the data root's filesystem permissions, which on
supported platforms are user-private by default.

## 8. Versioning

| Schema                       | Version constant                              |
|------------------------------|-----------------------------------------------|
| `ProvenanceRowV1`            | `PROVENANCE_ROW_SCHEMA_VERSION = 1`          |
| `WorkingTreeSnapshotV1`      | `WORKING_TREE_SNAPSHOT_SCHEMA_VERSION = 1`   |
| JS twin `PROVENANCE_KIND`    | `"git_read"`                                  |
| JS twin schema version       | `PROVENANCE_SCHEMA_VERSION = 1`              |

A bump to any version constant is a breaking change for downstream
readers. The runtime, the JS twin, and this document must be updated
in the same change.
