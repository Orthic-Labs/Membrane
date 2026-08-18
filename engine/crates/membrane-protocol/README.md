# membrane-protocol

The **contract source of truth** for the Membrane protocol's five typed shapes.

| Shape               | Rust type                 | JSON Schema                                       | Golden fixture                                  |
|---------------------|---------------------------|---------------------------------------------------|-------------------------------------------------|
| ScopeGrant          | `ScopeGrantV1`            | `schemas/scope-grant.v1.schema.json`              | `schemas/registry/scope-grant.v1.golden.json`         |
| ContextCandidateSet | `ContextCandidateSetV1`   | `schemas/context-candidate-set.v1.schema.json`    | `schemas/registry/context-candidate-set.v1.golden.json` |
| ContextPacket       | `ContextPacketV1`         | `schemas/context-packet.v1.schema.json`           | `schemas/registry/context-packet.v1.golden.json`      |
| ContextReceipt      | `ContextReceiptV1`        | `schemas/context-receipt.v1.schema.json`          | `schemas/registry/context-receipt.v1.golden.json`     |
| KnowledgeEmission   | `KnowledgeEmissionV1`     | `schemas/knowledge-emission.v1.schema.json`       | `schemas/registry/knowledge-emission.v1.golden.json`  |

The Rust types in `src/types.rs` are authoritative. Everything else — the JSON
Schemas, the golden fixtures, the TypeScript binding — is derived from and
pinned to them.

## Layout

- `src/types.rs` — the five shapes (serde, `camelCase`, `deny_unknown_fields`).
- `src/canonical.rs` — the canonical-byte serializer (`canonicalize`) and the
  `sha256:` digest. This is the cross-language byte contract.
- `src/lib.rs` — the `SHAPES` registry embedding each schema + fixture, and the
  `CanonicalSerialize` trait (`.canonical_json()` / `.canonical_digest()`).
- `tests/roundtrip.rs` — per-shape: schema-validate the fixture, deserialize to
  the Rust type, re-serialize, and require byte-identical canonical form; plus a
  pinned-digest test.
- `tests/common/mod.rs` — a dependency-free minimal JSON-Schema validator.
- `bindings/protocol.mjs` + `bindings/roundtrip.test.mjs` — the dependency-free
  TypeScript/JS binding and its `node --test` suite.

## Verify

```sh
cargo test -p membrane-protocol
node --test engine/crates/membrane-protocol/bindings/roundtrip.test.mjs
```

Both suites read the **same** fixture files and assert the **same** canonical
`sha256:` digests, so a contract drift fails on both sides. See
[`docs/protocol/source-of-truth.md`](https://github.com/Orthic-Labs/Membrane/blob/main/docs/protocol/source-of-truth.md)
for the full story.
