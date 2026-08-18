# Operation-specific contracts and error taxonomies (MBR-301)

`engine/crates/membrane-protocol` is the contract source of truth for the
five typed shapes (`ScopeGrantV1`, `ContextCandidateSetV1`,
`ContextPacketV1`, `ContextReceiptV1`, `KnowledgeEmissionV1` — see
`docs/protocol/source-of-truth.md`). The same crate is also the source of
truth for the **per-operation MCP contracts**: every tool the Membrane MCP
server exposes gets its own independently-versioned schema, its own closed
typed error taxonomy, and its own golden success and error fixtures.

## Why per-operation contracts

A single global schema for the whole MCP surface cannot keep up with the
real ownership of the tool surface:

- `membrane_context` needs to type its workspace fan-out, scope grant,
  envelopes, and 10+ specific failure modes.
- `membrane_knowledge_propose` needs a `LifecycleReceiptV1` shape that
  has nothing to do with `membrane_feedback` even though both produce a
  receipt.
- `membrane_working_context` is a discriminated save/load/close
  operation whose failure modes are a strict subset of the working-context
  surface — not the whole server.

Forcing one schema makes every change either a breaking change to every
consumer or a relaxed-any contract that no one can validate. Per-operation
schemas with **independent versioning** is the alternative: each tool's
contract advances on its own clock, and one operation's drift never moves
a sibling.

## The contract envelope

Every per-operation schema (`schemas/operations/<op>.v1.schema.json`)
defines the same envelope:

```json
{
  "schemaVersion": 1,          // INDEPENDENT contract version of this operation
  "operation": "membrane_x",   // closed discriminator (one tool per schema)
  "errorVersion": 1,           // INDEPENDENT error-taxonomy version
  "result": { "oneOf": [success, error] }
}
```

- `schemaVersion` advances when the input / success-output shape changes.
- `errorVersion` advances when the closed set of typed error codes changes.
- The two are **independent**: tightening an error code list does not
  move `schemaVersion`, and adding a new output field does not move
  `errorVersion`.

The success and error branches both share the same
`schemaVersion` + `operation` envelope, so a caller that does not yet
know an operation's `errorVersion` can still tell success from error by
`result.kind`.

## The success and error branches

- `result.kind: "success"` carries `data: <operation-specific output>`,
  validated against the schema's `#/$defs/<op>Output` block.
- `result.kind: "error"` carries the typed error envelope:

  ```json
  {
    "kind": "error",
    "code": "<closed_taxonomy_code>",   // e.g. "context_deadline_exceeded"
    "message": "<human-readable one-liner>",
    "retryable": true | false,
    "details": { ... }                 // optional, operation-specific
  }
  ```

  `code` is a closed enum in the schema. Adding a new code is an
  `errorVersion` bump and a contract change, not a silent expansion.

## Independent-versioning registry

The cross-operation registry is the only artifact that observes the
whole set:

- Schema: `schemas/operations/operations-index.v1.schema.json`
- Fixture: `schemas/registry/operations/operations-index.v1.golden.json`

Each entry in `operations` carries:

- `name` — the stable MCP tool name.
- `schemaVersion` — the operation's independent contract version.
- `errorVersion` — the operation's independent error-taxonomy version.
- `schemaPath` — repo-relative path of the per-operation JSON Schema.
- `successFixture` / `errorFixture` — repo-relative paths of the
  golden fixtures (success / typed-error).
- `errorCodes` — the closed list of error codes this operation
  defines. The contract test asserts every code in this list is also
  literally present in the per-operation schema's
  `#/$defs/errorCode/properties/code/enum`.

## Source of truth and round-trip

The Rust side lives in `engine/crates/membrane-protocol/src/operations.rs`
(constants: `OPERATIONS`, types: `OperationSpec`, `OperationResult`,
`OperationsIndex`, `OperationIndexEntry`, `ResultKind`). The TypeScript
side lives in
`engine/crates/membrane-protocol/bindings/operations.mjs` (constant:
`OPERATIONS`, helpers: `validateOperationFixtures`). Both sides load
the same on-disk schemas and fixtures and reproduce the same validation
results.

Two tests lock the contract in place:

- Rust: `engine/crates/membrane-protocol/tests/operations_roundtrip.rs`
  — every fixture validates, every fixture deserializes into the
  correct `OperationResult` variant, the Rust `OPERATIONS` registry
  matches the on-disk `operations-index.v1` fixture, and the canonical
  digest of the index is pinned.
- TypeScript:
  `engine/crates/membrane-protocol/bindings/operations.test.mjs` —
  same four properties, plus the index digest is pinned to the same
  value the Rust side pins.

A drift in the Rust registry, the on-disk JSON, the canonical rules,
or the closed taxonomy fails **both** suites. That is the visible
contract story MBR-301 requires: every operation has golden
success/error fixtures and independent versioning, and that contract is
enforced by tests that run at the Book 1 gate.

## How to add a new operation

1. Write `schemas/operations/<op>.v1.schema.json` with the
   `schemaVersion` / `operation` / `errorVersion` envelope and the
   `result.oneOf: [success, error]` shape.
2. Write `schemas/operations/operations/<op>.v1.golden.json` and
   `schemas/operations/operations/<op>.v1.error.golden.json` — valid instances
   of the success and error branches respectively.
3. Add an entry to `schemas/registry/operations/operations-index.v1.golden.json`
   with the new operation's `name`, `schemaVersion`, `errorVersion`,
   `schemaPath`, `successFixture`, `errorFixture`, and `errorCodes`.
4. Add a matching entry to `OPERATIONS` in
   `engine/crates/membrane-protocol/src/operations.rs` and
   `engine/crates/membrane-protocol/bindings/operations.mjs`.
5. Recompute the operations-index canonical digest (`sha256` over the
   canonicalized JSON) and update the pin in both
   `tests/operations_roundtrip.rs` and
   `bindings/operations.test.mjs`.

Steps 1, 2, and 3 are the contract; 4 and 5 are the lock-step
enforcement. Skipping step 4 leaves the registry drifting; skipping
step 5 leaves a silent digest drift that hides the divergence.

## Cross-references

- Per-shape (the five typed shapes): `docs/protocol/source-of-truth.md`.
- Source-of-truth crate: `engine/crates/membrane-protocol/src/lib.rs`.
- MBR-101 round-trip tests: `engine/crates/membrane-protocol/tests/roundtrip.rs`
  and `engine/crates/membrane-protocol/bindings/roundtrip.test.mjs`.
- MBR-301 round-trip tests: `engine/crates/membrane-protocol/tests/operations_roundtrip.rs`
  and `engine/crates/membrane-protocol/bindings/operations.test.mjs`.
- MBR-301 public contracts entrypoint:
  `tests/contracts/operations-contract.test.mjs`.
