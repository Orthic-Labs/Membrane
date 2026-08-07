# Membrane Stable Roots

MBR-106 standardizes the four durable directories the runtime reads and
writes. Every installer, updater, rollback, and uninstaller anchors on
this contract so user data survives and uninstall only removes files
the runtime itself placed.

## The four roots

| Root     | Purpose                                          | Survives update | Survives uninstall |
|----------|--------------------------------------------------|-----------------|--------------------|
| `config` | durable settings, lease file, supervisor.json    | yes             | no (re-installed)  |
| `data`   | persistent memory DB, checkpoints, supervisor state | yes          | **yes** (user data) |
| `cache`  | regenerable caches (fastembed, downloaded models) | yes            | no                 |
| `log`    | append-only logs from resident and supervisor     | yes             | no                 |

The `data` root is the only one that holds user data the user expects
to survive uninstall. The other three are runtime-owned and may be
re-created by the next install.

## Platform mapping

Every root resolves through `membrane_runtime::paths` so the runtime,
the installer, and the `doctor paths` command agree byte-for-byte.

| Root   | Linux (XDG)                                | macOS                                          | Windows                  |
|--------|--------------------------------------------|------------------------------------------------|--------------------------|
| config | `$XDG_CONFIG_HOME/membrane` (or `~/.config`) | `~/Library/Application Support/Membrane`       | `%APPDATA%\Membrane`     |
| data   | `$XDG_DATA_HOME/membrane` (or `~/.local/share`) | `~/Library/Application Support/Membrane`   | `%LOCALAPPDATA%\Membrane`|
| cache  | `$XDG_CACHE_HOME/membrane` (or `~/.cache`) | `~/Library/Caches/Membrane`                    | `%LOCALAPPDATA%\Membrane`|
| log    | `$XDG_STATE_HOME/membrane/log` (or `~/.local/state`) | `~/Library/Logs/Membrane`             | `%LOCALAPPDATA%\Membrane`|

On macOS the `config` and `data` roots collapse onto the same
`~/Library/Application Support/Membrane` tree because macOS does not
distinguish "config" from "data" in user-visible terms. On Windows the
three non-config roots collapse onto `%LOCALAPPDATA%\Membrane`. The
runtime still treats them as separate logical roots so cache eviction
never touches data.

## Environment overrides

Each root honors one override variable, intended for tests and for
operators who want to pin a writable scratch tree without touching the
real install:

- `MEMBRANE_CONFIG_ROOT`
- `MEMBRANE_DATA_ROOT`
- `MEMBRANE_CACHE_ROOT`
- `MEMBRANE_LOG_ROOT`

When set, the override wins over every platform default. Empty
strings are treated as unset. The runtime never logs the override
value into the receipt — operators can confirm what was used by
running `membrane cli doctor paths`.

## How install / update / rollback / uninstall use the roots

- **Install.** The installer creates `config` and `log` from scratch,
  populates `cache` lazily as the resident warms up, and never writes
  to `data` (the engine DB starts empty).
- **Update.** The installer replaces the binary and the service unit.
  None of the four roots are touched. The receipt is read to learn
  which residue files the previous version wrote outside the roots;
  those are migrated or removed by the updater's policy.
- **Rollback.** Same as update, in reverse: the previous binary is
  restored, the four roots are left alone, and the receipt from the
  previous install is consulted so residue from a newer build does not
  leak into the older one.
- **Uninstall.** The installer removes files in `config`, `cache`, and
  `log`. It also unlinks every entry in the receipt
  (`membrane_runtime::receipt::snapshot`). It **never** touches `data`.
  If the user wants `data` removed, the uninstaller prints an explicit
  prompt and requires a second flag.

## The receipt and uninstall residue

Every file the runtime writes outside the four roots is recorded by
`membrane_runtime::receipt::register_receipt_owned`. The receipt is
held in process memory; the installer persists it as JSON next to the
uninstall marker so a future `membrane uninstall` can audit what to
remove without re-running the runtime.

`register_receipt_owned` rejects paths inside any of the four stable
roots — those writes are tracked by the roots themselves, and adding
them to the receipt would double-count on uninstall.

The receipt's `UninstallReceipt::capture()` snapshots the active
config root alongside the owned files, so the persisted JSON is enough
to reproduce the residue audit offline.

## Inspecting a live install

```
membrane cli doctor paths
```

prints the four resolved roots and every receipt-owned entry as JSON:

```json
{
  "schemaVersion": 1,
  "product": "Membrane",
  "roots": {
    "config": "/Users/alice/Library/Application Support/Membrane",
    "data":   "/Users/alice/Library/Application Support/Membrane",
    "cache":  "/Users/alice/Library/Caches/Membrane",
    "log":    "/Users/alice/Library/Logs/Membrane"
  },
  "receiptOwned": []
}
```

The installer uses the same command to decide whether the live
process's footprint matches the on-disk receipt. Any drift is a
finding the installer routes to the operator.

## Acceptance

> Install, update, rollback, and uninstall preserve user data and
> remove only receipt-owned files.

`membrane_runtime::paths` is the single source of truth for the four
roots; `membrane_runtime::receipt` is the single source of truth for
everything the runtime writes outside them. Install, update, rollback,
and uninstall all read from these two modules; nothing else owns the
stable-root contract.