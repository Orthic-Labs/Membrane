# Crypt (legacy engine name: Crypt)

Productizable, OS-independent memory engine (SQLite + vector + graph).

## Crates

- **crypt** — Binary/daemon: HTTP server, CLI, SQLite persistence, quantized vector store, hybrid retriever.
- **crypt-core** — Pure in-memory logic: memory tiers, retrieval ranking, embeddings, graph, eval gate, dream consolidation.
- **crypt-format** — Open Knowledge Format (OKF) bundle support and deterministic prose compression.
- **crypt-core**, **crypt-format**, **crypt-store** — staged Crypt namespace facades. They preserve existing Crypt IDs, SQLite schema, and replication history while imports migrate; `crypt*` remains the installed compatibility facade.

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

- Crate contract: [crates/crypt/SPEC.md](crates/crypt/SPEC.md)
- Engine architecture, families/layers, deploy lane: [tools/lib/CONTEXT-ENGINEERING.md](../lib/CONTEXT-ENGINEERING.md)
- Live deployment state + gate/evidence ledger: [docs/RIGHTCONTEXT-STATE.md](../../docs/RIGHTCONTEXT-STATE.md)

## Provenance

Migrated from `coderight/engine/crates/{crypt,memory,config}` at commit `e79c9984`. See `MIGRATION.md` for the full provenance chain.
