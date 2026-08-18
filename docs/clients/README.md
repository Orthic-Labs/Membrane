# Membrane client registry and support matrix

**Owner:** MBR-206 — `MBR-206: generate the client registry and support matrix`.

The client registry is the **canonical source of truth** for which AI / MCP
clients the Membrane runtime knows how to enroll, and which subset of
operations each client can honestly support. It is consumed by the
`membrane install` path, by the generated support matrix, and by every
operator-facing view of the Membrane client surface.

## Files

`operations/toolsets.yaml` is valid JSON-as-YAML discovery policy. Clients may
send `params._meta["membrane.toolsets.v1"]` to `tools/list`; invalid requests
fall back to `membrane_context`. Native discovery negotiates same metadata but
advertises no tools until native tool execution exists.

| Path | Purpose |
|---|---|
| `operations/clients.yaml` | Human-authored registry — one row per client. |
| `operations/clients.capabilities.v1.json` | Generated capability envelopes. Conforms to `schemas/client-capability.v1.schema.json`. |
| `docs/clients/support-matrix.v1.json` | Generated client × operation matrix. Conforms to `schemas/client-support-matrix.v1.schema.json`. |
| `schemas/client-capability.v1.schema.json` | JSON Schema for one capability envelope. |
| `schemas/client-support-matrix.v1.schema.json` | JSON Schema for the matrix. |
| `scripts/tools/productization/generate-client-matrix.mjs` | Generator (byte-stable). |
| `mcp/install.mjs` | Exposes `loadClientCapabilities`, `loadSupportMatrix`, `clientsForEnrollment`, and `supportedOperationsFor` for the install path. |
| `tests/clients/client-matrix.test.mjs` | Contract test for the registry. |

## Declared clients (current revision)

| `id` | Display name | Transport | Discovery | Honest level | Operations supported |
|---|---|---|---|---|---|
| `claude` | Claude Code | stdio | native-mcp-cli | L4 | all nine |
| `codex` | Codex CLI | stdio | native-mcp-cli | L2 | all nine |
| `cursor` | Cursor | stdio | rules-file-and-mcp | L1 | context, working_context |
| `windsurf` | Windsurf | stdio | rules-file-and-mcp | L1 | context, working_context |
| `generic_mcp` | Generic MCP client | stdio | mcp-stdio-only | L0 | context |

The nine operations referenced by the matrix come from
`operations/operations/operations-index.v1.golden.json` (MBR-301):

- `membrane_context`
- `membrane_source_read`
- `membrane_knowledge_propose`
- `membrane_checkpoint_save`
- `membrane_checkpoint_load`
- `membrane_working_context`
- `membrane_temporal_fact`
- `membrane_scratchpad`
- `membrane_feedback`

The matrix is the cartesian product of these five clients and nine
operations (45 cells). Every cell is one of `supported`, `degraded`, or
`unsupported`. `degraded` means the client can call the operation but
cannot enforce its contract; `unsupported` means the client cannot
legitimately use the operation at all.

## Adding a new client

1. Append a new entry under `clients:` in
   `operations/clients.yaml`. Every required key is mandatory:

   | Key | Meaning |
   |---|---|
   | `id` | Stable lowercase snake_case identifier. Must be unique across clients. |
   | `display_name` | Human-facing name shown in operator output. |
   | `transport` | `stdio` (native MCP CLI launches the server) or `loopback` (client connects to a per-user supervisor port). |
   | `discovery_method` | Free-form string describing how the install path confirms the client is present (e.g. `native-mcp-cli`, `rules-file-and-mcp`, `mcp-stdio-only`). |
   | `install_command` | Reference install command template; `<node>` and `<server>` are substituted at run time. |
   | `detect_command` | Argument vector used to confirm the binary exists (typically `--version`). |
   | `binary_env` | Optional env var that overrides the client binary path (e.g. `MEMBRANE_CURSOR_BIN`). |
   | `scope_default` | Default install scope (e.g. `local`, `project`, `user`). |
   | `honest_level` | Highest Membrane honesty level the client can deliver end-to-end (`L0`–`L5`). |
   | `authority_grant` | Largest authority the install path may grant: `max_level`, `scopes`, `loopback_only`. |
   | `supported_operations` | Operations the client honors end-to-end. Every name must exist in the operation index. |
   | `degraded_operations` | Operations the client can call but not enforce. |

2. The contract test (`tests/clients/client-matrix.test.mjs`) verifies the
   registry. The generator refuses to emit if any required key is missing
   or if a `supported_operations` / `degraded_operations` entry is unknown.

3. Regenerate the artifacts:

   ```sh
   node scripts/tools/productization/generate-client-matrix.mjs
   ```

   The two artifacts are byte-stable; the diff against the previous
   revision should be a clean addition of the new client and the new
   matrix cells.

4. Commit the registry YAML, the regenerated JSON artifacts, and any test
   updates together. Do not edit the generated JSON files by hand —
   `stableStringify` in the generator is the only writer.

## How install pulls the matrix

`mcp/install.mjs` exposes four additive helpers:

```js
import {
  loadClientCapabilities,
  loadSupportMatrix,
  clientsForEnrollment,
  supportedOperationsFor,
} from "./mcp/install.mjs";

const caps = await loadClientCapabilities();
const matrix = await loadSupportMatrix();
const ids = await clientsForEnrollment();             // ["claude", "codex", "cursor", "windsurf", "generic_mcp"]
const claudeOps = await supportedOperationsFor("claude"); // all nine operations
```

The install CLI itself does not yet call these helpers — they are the
read-only surface the future `membrane install --client <id>` paths
will use to validate that a request is well-formed against the registry.
Adding the dispatch logic is the responsibility of MBR-203 / MBR-207.

## Schema constraints

Both schemas use `additionalProperties: false` and pin `schemaVersion`
to a `const`. The matrix schema pins both `schemaVersion` and
`matrixVersion` so future shape changes can advance the matrix version
without breaking clients pinned to v1.

## See also

- `docs/operations/supervisor.md` — the per-user Membrane supervisor
  that owns the loopback port clients reuse.
- `docs/installation/contract.md` — the resident ↔ client handshake.
- `docs/membrane/capability-matrix.v1.json` — the older host capability
  matrix the MBR-206 registry supersedes for the client-side view.
