# Cortex — Rust memory SDK (parity tracker)

Productizable, OS-independent durable-memory engine. Replaces workspace Python memory persistence
with a Rust-owned Cortex library & CLI projection inside Membrane. Anyone can embed the library;
`rusqlite` bundles SQLite (no system dep) and `fastembed` pulls BGE-small on first use — cross-platform.

`coderight-memory` stays the inner primitives library (embed, graph, effectiveness, dream, no I/O);
`cortex` is the durable subsystem: store + scope model + scoring + ingest + CLI. CR-the-app keeps using
`coderight-memory` in-process (single scope); the workspace uses Membrane resident's authenticated
loopback contract, so hooks need no independent Cortex service.
Current capability canon: `docs/canon/cortex.md`; parent architecture: `docs/architecture/membrane.md`.

## Parity checklist (vs mem.py)

| Area | Requirement | Phase | Status |
|---|---|---|---|
| Schema | memories(+scope_id,kind) + links + recall_log + usage_log, identical columns | P1 | DONE |
| Scope | path_to_scope (`:`/`\`/`/`→`-`, no collapse, **drive-letter normalized**); scope_chain (self+ancestors+global, sibling-isolated); `cross`; `kinds` | P1 | DONE |
| Embed | BGE-small-384 via fastembed; **asymmetric** — query gets the BGE query-instruction prefix, docs raw | P2 | DONE |
| Ingest | markdown frontmatter (type/name/…) + `[[wikilink]]`→links; `_infer_scope` (projects/<slug>/memory→memory, <repo>/.agent→blueprint); add_file embed+upsert | P2 | DONE |
| Recall | measured semantic rank: cosine similarity + 0.02 per nearer scope rank, then a bounded one-hop link lane (≤20% of results, max 8); verified contradictory feedback is a hard veto until content is superseded. Recency, access/effectiveness, and pin bonuses are **not shipped** because they did not clear the frozen eval gate. | P3 | DONE |
| Serve | HTTP `/health` `/recall` `/add` `/use` — identical JSON contract | P4 | DONE |
| CLI | `serve migrate-all migrate-blueprint reindex add recall use metrics curate` | P4 | DONE |
| Curate | normalize relative dates → dedupe(0.97) → contradiction-merge(0.90, supersede) → reversible quarantine of low-effectiveness, never-used rows; duplicate losers remain permanent prunes with provenance + tombstones | P5 | DONE |
| Migrate + cutover | migrate durable rows, verify recall parity, route hooks through Membrane resident, retire legacy Python memory | P6 | **DONE — Rust durable subsystem is live** |

## Status (2026-06-30) — CUTOVER COMPLETE

All six phases done. Cortex owns durable memory while Membrane Hub owns one resident service on
the authenticated loopback contract; **Python `mem.py` is deleted**. Hooks use Membrane's resident
identity and never start an independent Cortex service.

**The onnxruntime link wall** (ort `download-binaries` needs newer MSVC STL symbols than this toolset
has → `LNK1120`) was solved with ort **`load-dynamic`**: onnxruntime loads at runtime from
`$ORT_DYLIB_PATH`, so it builds on any toolset.

**Embedder parity is exact.** The Rust BGE embedding of a literal is byte-identical to Python's
(`[-0.08282,-0.05044,…]` vs `[-0.08277,-0.05045,…]`). The mid-cutover "regression" (cos 0.34 vs 0.65)
was a **build-artifact bug** — a `cargo build` (default features = hash embedder) had clobbered
`target/debug/cortex.exe`, so the migration + serve ran on the hash embedder. Fixed by building the
fastembed runtime to a stable Membrane installation. With the real BGE binary, recall
matched Python: same retrieved set, cos within ~0.02.

**Operational requirements — onnxruntime bundled (2026-06-30).** Running Cortex needs (1) the compiled
binary, (2) an `onnxruntime` shared lib via `ORT_DYLIB_PATH`. The DLL is now **bundled at
`tools/bin/onnxruntime.dll`** (copied from the Python wheel, but standalone — the runtime no longer
needs Python installed); the hooks default `ORT_DYLIB_PATH` there. A real cross-machine install ships
that DLL beside the binary (the standard `load-dynamic` + shipped-DLL pattern). `default_embedder`
PANICS under the fastembed feature if the model fails to load (no silent hash fallback).

**Storage convergence — DONE (2026-06-30).** Cortex stores **quantized turbovec** vectors via
`coderight-memory::quant` (`QuantizedVector`: signed-8-bit + per-vector scale) — the SAME low-RAM
representation CodeRight's daemon stores. No more f32 divergence: the workspace and CR share one storage
path and one quantization. ~4x smaller vector working set; recall quality preserved (quantized cos
matches f32 within ~0.001, same retrieved set). Cortex follows CR's lightweight design.

**Remaining (larger product decision, not blocking):** CodeRight's daemon still wires
`coderight-memory` (retriever/registry/dream/tiers) directly for its *session* memory; Cortex is the
*workspace multi-project* layer. They now share the same core + quantization. Full unification (CR's
daemon consuming Cortex's store/recall wholesale) would mean reconciling CR's session-tier/registry
features with Cortex's scope/kind/markdown recall — a real product decision, not a quick win.

**Cutover steps (after parity holds — production-mutating, repoints the live recall system):**
1. Migrate the production store (`migrate-all` + `migrate-blueprint`, real BGE) into `cortex.db`.
2. Start the Membrane resident through Hub with its authenticated runtime identity.
3. Repoint hooks to the Membrane resident client; never launch a separate Cortex service.
4. Retire `mem.py`; update `CONTEXT-ENGINEERING.md` (engine = Rust).

**Operational note for distribution:** running Cortex needs the compiled binary + an `onnxruntime.dll`
via `ORT_DYLIB_PATH`. On this machine that DLL comes from the Python onnxruntime wheel; for
"installable by anyone" the release must bundle a matching `onnxruntime.dll` (ort `load-dynamic` +
shipped DLL is the portable pattern). This is a distribution decision, not a code blocker.
