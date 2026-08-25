# Workspace mode vs. product mode

Membrane ships as **two deployment topologies**. They never share a label
namespace, never share a path tree, and both may coexist on one machine. This
page is the disambiguation reference — link here instead of re-deriving the
boundary.

## Workspace mode (what actually runs today)

Binaries: `membrane` / `cortex`. Both are stateless clients; the active Hub
process hosts the sole runtime. `cortex` is Cortex's durable-memory CLI.

Resolved by `deployed_runtime_from_exe()` in
`engine/crates/membrane-runtime/src/cli.rs:242-278`, which recognizes
**only** a tree shaped:

- `<root>/tools/bin/{membrane,cortex}`
- `<root>/tools/lib/memory/runtime.json` (`serviceId: "membrane-local-v1"`)
- `<root>/tools/.cache/memory/{cortex-engine.db,api-token}`
- `<root>/tools/.cache/fastembed`

Resident transport uses `MEMBRANE_PORT` & `MEMBRANE_API_TOKEN_FILE`.

Root is declared via `WORKSPACE_ROOT` / `MEMBRANE_WORKSPACE_ROOT`. Any tree
not shaped exactly this way is not recognized as a deployed workspace
runtime — there is no fuzzy match.

Service labels:

- macOS (launchd): `com.membrane.workspace.cortex-serve`,
  `com.membrane.workspace.cortex-daily`, `com.membrane.workspace.cortex-replication`
- Windows (Task Scheduler): `\Membrane\Workspace\...`

Installer: `membrane/install/workspace/` — this is the owning location for
workspace-mode install/service-registration logic (currently being built
out; do not treat its absence of macOS/Windows subtrees as a gap to fill
ad hoc).

**Platform parity is uneven, on purpose — do not assume symmetry:**

- Linux daily-sync / replication scheduling does not exist today, in any
  form (no cron/systemd-timer equivalent has been written).
- Windows blueprint-watch registration is unimplemented.

Treat both as open gaps, not silent no-ops.

## Product mode (real, in progress, not yet deployed)

Binaries: `membrane`; the Hub links and hosts the runtime in-process.

Paths are OS-standard per-user locations, not a workspace tree:

- macOS: `~/Library/Application Support/Membrane`
- Linux: XDG base dirs
- Windows: `%APPDATA%`

Assets live in `membrane/install/{macos,linux,windows}`. Membrane Hub owns
resident lifecycle; no second product-service label or binary exists.

This is **MBR-201 / MBR-203 / MBR-208** — active, not abandoned. Its install
assets exist and ship, but product mode is not yet the deployed runtime
 anywhere; workspace mode is what machines actually run today. See
[`docs/operations/resident-lifecycle.md`](resident-lifecycle.md) for the
in-process resident lifecycle, and
[`docs/installation/contract.md`](../installation/contract.md) for the
per-mode installation manifest contract that both topologies must satisfy.

## The one rule

The canonical on-disk layout for each mode (workspace: the `tools/bin` +
`tools/lib/memory` + `tools/.cache/memory` shape above; product: the
OS-standard per-user paths) is a **versioned Membrane-owned contract**, not
an implicit convention someone can shift by moving a file. Changing either
shape is a contract change, not a refactor — it must update
`deployed_runtime_from_exe()` (workspace) or Hub resident path resolution
(product) and this document together.
