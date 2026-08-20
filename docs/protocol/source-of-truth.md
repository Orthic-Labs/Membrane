# membrane-protocol: the contract source of truth

`engine/crates/membrane-protocol` is the single source of truth for the
Membrane protocol's five typed shapes — the contract every provider, adapter,
and client agrees on so database formats, parsers, and local paths never leak
across the wire:

- **ScopeGrant** (`ScopeGrantV1`) — bounded, Ed25519-signed authority to read
  exact source ranges for one task.
- **ContextCandidateSet** (`ContextCandidateSetV1`) — the federated candidates
  fed to the deterministic admission planner.
- **ContextPacket** (`ContextPacketV1`) — the bounded packet the agent receives.
- **ContextReceipt** (`ContextReceiptV1`) — the content-free record of what
  entered context, what didn't, and why.
- **KnowledgeEmission** (`KnowledgeEmissionV1`) — a bounded, typed proposal to
  persist durable memory.

This crate is the foundational contract for the Membrane Wave 1/2 work: every
later task builds on these types rather than re-deriving them.

## Rust types are authoritative

The shapes are defined once, in Rust, in
`engine/crates/membrane-protocol/src/types.rs`. Field names, optionality, and
JSON casing are modelled faithfully on how the shapes are produced and consumed
today:

- `ScopeGrantV1` mirrors `mcp/scope-grant-v1.mjs` (`mintScopeGrantV1`).
- The candidate-set / packet / receipt shapes mirror the admission planner in
  `cortex-core::planner` (which itself mirrors the versioned contract schema).
- `KnowledgeEmissionV1` mirrors the `membrane_knowledge_propose` body persisted
  verbatim by `ProposalStore`.

Serde conventions make Rust ↔ JSON ↔ TypeScript agree: every struct uses
`rename_all = "camelCase"`; closed shapes use `deny_unknown_fields`; optional
fields are `default + skip_serializing_if = "Option::is_none"` so they are
absent from JSON when `None` (never explicit `null`); enums serialize as
`snake_case` strings.

## How the schemas are generated

The canonical JSON Schema (draft 2020-12) document for each shape lives under
`schemas/` (e.g. `schemas/scope-grant.v1.schema.json`). Each schema is derived
from the corresponding Rust type and hand-pinned to match the serde JSON shape
exactly — `required`/`properties`/`additionalProperties: false` mirror the
struct fields, and each enum's `enum` mirrors the Rust variants. There is no
heavy `schemars` dependency: the schema is the canonical contract text, kept in
lock-step with the Rust type by the round-trip tests below.

## How the round-trip is enforced

Golden fixtures under `schemas/operations/` (one per shape, `*.v1.golden.json`) are the
shared canonical instances. Both sides read the **same** files and assert the
**same** canonical `sha256:` digest:

- **Rust** — `cargo test -p membrane-protocol`. For each shape it
  (1) validates the fixture against its JSON Schema,
  (2) deserializes the fixture into the Rust type and re-serializes it,
      requiring byte-identical canonical form, and
  (3) checks the canonical digest against a pinned value
      (`tests/roundtrip.rs::canonical_digests_are_pinned`).
- **TypeScript/JS** — `node --test engine/crates/membrane-protocol/bindings/roundtrip.test.mjs`.
  The dependency-free binding (`bindings/protocol.mjs`) loads the same fixture
  and schema, validates with the same minimal schema subset, and asserts the
  same pinned digest.

### The canonical-byte contract

Cross-language agreement reduces to one deterministic serialization, defined in
`src/canonical.rs` (`canonicalize`) and mirrored in `bindings/protocol.mjs` (and
pre-existing in `mcp/scope-grant-v1.mjs`):

1. Object keys sorted lexicographically.
2. No insignificant whitespace.
3. Present-but-`null` fields preserved (distinct from absent).
4. Integers bare; the contract's float fields use shortest-round-trip values
   (e.g. `0.92`, `0.5`) where JS `JSON.stringify` and Rust `serde_json` emit the
   identical shortest form.

Because both sides canonicalize-then-digest the same bytes, the `sha256:` digest
is the single value that proves Rust and TypeScript agree. A drift in the Rust
types, the fixtures, or the canonical rules fails the pinned-digest assertion on
**both** sides.

## Changing the contract

Make the change in `src/types.rs`, regenerate/update the matching schema under
`schemas/` and the golden fixture under `schemas/operations/`, then update the pinned
digest in **both** `tests/roundtrip.rs` and `bindings/roundtrip.test.mjs`.
Digest drift is a deliberate, visible failure — never a silent one.
