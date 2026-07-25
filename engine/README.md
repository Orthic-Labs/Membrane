# MemRight

Productizable, OS-independent memory engine (SQLite + vector + graph).

## Crates

- **memright** — Binary/daemon: HTTP server, CLI, SQLite persistence, quantized vector store, hybrid retriever.
- **memright-core** — Pure in-memory logic: memory tiers, retrieval ranking, embeddings, graph, eval gate, dream consolidation.
- **memright-format** — Open Knowledge Format (OKF) bundle support and deterministic prose compression.

## Build

```bash
cargo build --workspace
cargo test --workspace
```

With real ONNX embeddings:

```bash
cargo test --workspace --features fastembed
```

## Docs

- Crate contract: [crates/memright/SPEC.md](crates/memright/SPEC.md)
- Engine architecture, families/layers, deploy lane: [tools/lib/CONTEXT-ENGINEERING.md](../lib/CONTEXT-ENGINEERING.md)
- Live deployment state + gate/evidence ledger: [docs/RIGHTCONTEXT-STATE.md](../../docs/RIGHTCONTEXT-STATE.md)

## Provenance

Migrated from `coderight/engine/crates/{memright,memory,config}` at commit `e79c9984`. See `MIGRATION.md` for the full provenance chain.
