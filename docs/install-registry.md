# MCP Registry metadata (MBR-907)

> See also: [installation/](installation/) for the manifest/IPC handshake contract and stable-roots reference.

`server.json` at the repo root is the metadata this repo would submit to the
official [MCP Registry](https://github.com/modelcontextprotocol/registry)
under the reserved server name `io.github.orthic-labs/membrane`, once a real
release exists to publish. **No submission has happened.** This task
publishes no metadata anywhere; it only makes the metadata this repo would
submit correct, honest, and mechanically checked against the real repo state.

## What `server.json` asserts, and how each part is checked

`node scripts/release/registry/verify-server-json.mjs` runs the full check
(read-only; it publishes, signs, or submits nothing):

1. **Identity** — `server.name`, `server.npm.mcpName`, and
   `dist/npm/package.json`'s `mcpName` all equal
   `io.github.orthic-labs/membrane`; `server.npm.package` and
   `dist/npm/package.json`'s `name` both equal `@orthic/membrane`; `server.version`
   equals `dist/npm/package.json`'s `version` and is valid semver; the repository
   URL matches on both sides; and `server.npm.install` pins the exact
   `npm:@orthic/membrane@<version>` route. This half is delegated to
   `scripts/release/verify-mcp-registry.mjs` (a prior, unmodified pass — see
   below), not re-implemented here.
2. **Publication state** — `server.publication.namespaceStatus` and
   `.artifactStatus` must both be `"published"`, with a real
   `namespaceReceipt` (`receiptId` + `sha256`), before the verifier accepts
   the checkout as installable. **Today both are `"unpublished"`/`"unverified"`
   with `namespaceReceipt: null`, and the verifier fails closed on exactly
   that** (`tests/mcp-registry/registry.test.mjs`,
   `tests/mcp-registry/verify-server-json.test.mjs`). This is the correct,
   current state: no MCP Registry namespace reservation and no npm publish
   have happened. Reserving `io.github.orthic-labs/membrane` in the actual
   MCP Registry, and running `npm publish` for `@orthic/membrane` and its six
   platform packages, are the deliberately separate, credentialed, manual
   actions this task's hard rules forbid it from performing or automating.
3. **Native artifact evidence** — for each of the six `dist/npm/platforms/**`
   packages (`darwin-arm64`, `darwin-x64`, `linux-arm64`, `linux-x64`,
   `win32-arm64`, `win32-x64`), `server.json`'s `nativeArtifacts` entry names
   the matching package and version. Every `sha256`, `signature`, and
   `platformReceipt` field is `null` today — declared placeholders, not
   plausible-looking fakes — because no signed native artifact has been
   produced by this checkout's release pipeline (MBR-901–MBR-906) yet. When
   `requirePublished` is true, the verifier requires a real
   `sha256:<64-hex>` digest, an `ed25519` signature bound to that digest, and
   (for `darwin-*`/`win32-*`) a platform receipt id, before it will accept
   the artifact as installable.
4. **Tool contract coverage (new in this task)** — `server.json`'s `tools`
   array. See below.

## Tool contract coverage, and the `membrane_blueprint` gap

`scripts/release/registry/tool-contract-coverage.mjs` computes, directly
from the two real live modules (never hand-typed, never a static list this
task authored independently):

- `TOOLS` in `mcp/server.mjs` — the exact tool list the running MCP server
  advertises via `tools/list`.
- `OPERATIONS` in
  `engine/crates/membrane-protocol/bindings/operations.mjs` — the
  cross-operation registry that pins a `schemaVersion`/`errorVersion` pair,
  golden success/error fixtures, and a closed error-code taxonomy
  (validated by `validateOperationFixtures`) per operation.

Every tool name present in `TOOLS` **and** in `OPERATIONS` is marked
`"contractCoverage": "operations_registry"` in `server.json`. A tool present
in `TOOLS` but **absent** from `OPERATIONS` is marked
`"contractCoverage": "gap"` with a `gapReason` string, instead of being
silently reported as fully covered.

**`membrane_blueprint` is that gap today.** It is a real, dispatchable MCP tool
(`mcp/server.mjs`'s `TOOL_DEFINITIONS`, dispatched at
`if (name === "membrane_blueprint")`), but it has no matching entry in
`OPERATIONS` — no `schemaVersion`/`errorVersion`, no golden success/error
fixture pair under `schemas/operations/operations/`, and no closed error-code
taxonomy the MBR-301 contract machinery validates. This task does not add
golden fixtures or an error taxonomy for `membrane_blueprint` (that is
`membrane_blueprint`'s own contract work, outside this task's allowed paths);
it only ensures the registry metadata reports that gap honestly rather than
presenting `membrane_blueprint` as equivalent to the other nine tools.

`scripts/release/registry/verify-server-json.mjs` enforces this mechanically:
it recomputes the expected tool list live and rejects `server.json` if the
declared list differs in membership, ordering, `contractCoverage` value, or
`gapReason` presence — including the specific case of someone marking
`membrane_blueprint` `"operations_registry"` without adding real fixtures.
`tests/mcp-registry/tool-contract-coverage.test.mjs` and
`tests/mcp-registry/verify-server-json.test.mjs` prove this both against the
real repo and against synthetic before/after registries, so a future change
that adds an eleventh tool, removes a tool, or closes the `membrane_blueprint`
gap must update `server.json` in the same change or these tests fail.

## What this task did not do

- No `npm publish`, no MCP Registry submission, no signing, no
  package/version invention. Every identifier in `server.json` is either
  copied from `dist/npm/package.json` (already landed by MBR-906) or copied from
  the live `mcp/server.mjs`/`operations.mjs` source via
  `tool-contract-coverage.mjs`.
- No golden fixtures or error taxonomy were added for `membrane_blueprint`;
  that would require changes to `engine/crates/membrane-protocol/**` and
  `schemas/operations/operations/**`, both outside this task's allowed paths (see
  `MBR-401`'s allowlist note in
  `MEMBRANE-BOOK-MODE-EXECUTION-RULES.md` for where that work belongs).
- The manifest command `node scripts/release/verify-release-plan.mjs --task
  MBR-907` was not run: that script does not exist in this checkout (it is
  created by a later, gate-owning change), and per
  `MEMBRANE-BOOK-MODE-EXECUTION-RULES.md`, per-task manifest command
  execution and `acceptance.json.status: "pass"` are reserved for the Book 3
  final phased gate.
