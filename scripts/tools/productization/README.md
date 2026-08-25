# scripts/tools/productization/

Deterministic, locally invoked productization utilities. Nothing in this directory
adds or modifies `.github/workflows/**` or any CI configuration; every entry is
either a node script or a manual command runner.

Membrane Hub is the sole runtime/build/release/install authority. These tools
derive Membrane package, support, & product truth; they do not create a
parallel manifest, add-on, installer, or compatibility lane.

## Entries in this directory

| File | Owner task | Purpose |
|---|---|---|
| `generate-product-truth.mjs` | MBR-013 | Builds canonical `schemas/registry/product-truth.json` plus generated product docs. |
| `check-docs.mjs` | MBR-1001 | One-shot local gate that fails on stale generated docs or broken README links. |
| `render-docs.mjs` | MBR-1001 | Renderers for the four generated product docs. |
| `generate-client-matrix.mjs` | MBR-206 | Builds the MBR-206 client registry capabilities + support matrix. |
| `generate-support-matrix.mjs` | MBR-808 | Derives the published support-tier matrix from MBR-801 conformance receipts. |

## MBR-808 — generate-support-matrix.mjs

Reads:

- `docs/evidence/qualification/mbr801/<platform>/receipt.json` — real MBR-801
  installed-path conformance receipts (`membrane.mbr801-installed-receipt.v1`),
  produced by `node scripts/qualification/run.mjs` and validated here through
  the existing `scripts/qualification/verify-mbr801-evidence.mjs` (never
  re-implemented).
- `docs/clients/support-matrix.v1.json` — the MBR-206 client registry, for the
  canonical client-id universe. Read-only; this generator never writes it.

Emits:

- `docs/support-matrix.md` and `docs/support-matrix.json` — the platform ×
  client × feature tier table (`qualified` or `unavailable`, with a reason;
  never a fabricated `unsupported` state).
- The `<!-- support-matrix:start -->` / `<!-- support-matrix:end -->` block in
  `README.md`.
- `server.json`'s per-target `nativeArtifacts[*].platformReceipt` fields (the
  MCP Registry server descriptor), left untouched when nothing semantically
  changes so it never gets gratuitously reformatted.

A platform/client pair is `qualified` only when a receipt for that exact
platform verifies as `passed` for the current commit and release generation,
and that receipt names that exact client. Everything else — no receipt, a
stale commit, a different release generation, a different client, or a
malformed/incomplete receipt — renders `unavailable`.

### Run

```sh
node scripts/tools/productization/generate-support-matrix.mjs [--commit <sha>] [--release-generation <hex>]
```

Without `--release-generation` (no MBR-807/903 release evidence yet exists
in this repository), every row honestly renders `unavailable`.

### Programmatic use

```js
import { buildMatrix, render } from "../../scripts/tools/productization/generate-support-matrix.mjs";

const matrix = buildMatrix({ commit, releaseGeneration, clients, receiptPaths });
console.log(render(matrix));
```

## MBR-206 — generate-client-matrix.mjs

Reads:

- `schemas/registry/clients.yaml` — the human-authored client registry.
- `schemas/registry/operations/operations-index.v1.golden.json` — the MBR-301
  operation universe.

Emits:

- `schemas/registry/clients.capabilities.v1.json` — capability envelopes
  conforming to `schemas/client-capability.v1.schema.json`.
- `docs/clients/support-matrix.v1.json` — the client × operation
  matrix conforming to `schemas/client-support-matrix.v1.schema.json`.

Both artifacts are byte-stable across runs. The script refuses to emit
when the registry references an operation that does not exist in the
operation index, when a client id is duplicated, or when an operation
appears in both `supportedOperations` and `degradedOperations`.

### Run

```sh
node scripts/tools/productization/generate-client-matrix.mjs
```

Exits 0 with two `wrote …` lines on success, exits 1 with a thrown
contract error on failure.

### Programmatic use

The generator exports the same builder functions the companion test
imports, so `tests/clients/client-matrix.test.mjs` can verify the
generator without touching disk:

```js
import {
  buildArtifacts,
  parseYaml,
  stableStringify,
} from "../../scripts/tools/productization/generate-client-matrix.mjs";

const registry = parseYaml(yamlText);
const index = JSON.parse(indexText);
const { capabilities, matrix } = buildArtifacts(registry, index);
const sameAgain = stableStringify(buildArtifacts(registry, index).matrix);
assert.equal(stableStringify(matrix), sameAgain);  // byte-stable
```

### Contract checks the generator enforces

- Every `id` is unique across clients.
- Every `supportedOperations` / `degradedOperations` entry exists in
  the operation index.
- The same operation name never appears in both
  `supportedOperations` and `degradedOperations` for the same client.
- Every required envelope field is present (`id`, `displayName`,
  `transport`, `discoveryMethod`, `installCommand`, `honestLevel`,
  `authorityGrant`, `supportedOperations`, `schemaVersion`).

## Product-boundary truth

`docs/membrane/capability-matrix.v1.json` is the canonical capability input for
the six axes (`pull`, `push`, `cortex`, `blueprint`, `ledger`, `adapt`), current
supported target (`macOS`), Cortex scope (`durable-memory-only`), and resident
service authority (`hub`). `generate-product-truth.mjs` validates these
declarations and renders them into `schemas/registry/product-truth.json`,
`docs/product-truth.md`, and generated product docs. Drift or an incomplete
declaration fails generation and `--check`.

## Book-mode note

In book mode, this script is committed in the MBR-206 commit but the
manifest `commands` list (which contains `pnpm test` and Cargo
commands) is **not** executed at task time. The generator itself is
pure and side-effect-isolated; it is safe to invoke locally at any
point. The Book 1 gate is the first time the full verification runs.
