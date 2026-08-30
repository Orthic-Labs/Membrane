# Cortex

Productizable, OS-independent memory engine (SQLite + vector + graph).

## Crates

- **cortex** — Binary/daemon: HTTP server, CLI, SQLite persistence, quantized vector store, hybrid retriever.
- **cortex-core** — Pure in-memory logic: memory tiers, retrieval ranking, embeddings, graph, eval gate, dream consolidation.
- **cortex-format** — Open Knowledge Format (OKF) bundle support and deterministic prose compression.
- **cortex-core**, **cortex-format**, **cortex-store** — durable-knowledge logic, formats, & persistence.

## Build

```bash
rightkit cargo build --workspace
rightkit cargo test --workspace
```

With real ONNX embeddings:

```bash
rightkit cargo test --workspace --features fastembed
```

## Docs

- Crate contract: [crates/cortex/SPEC.md](crates/cortex/SPEC.md)
- Membrane architecture: [canonical doctrine](../docs/architecture/membrane.md)
- Cortex ownership: [atomic capability canon](../docs/canon/cortex.md)
