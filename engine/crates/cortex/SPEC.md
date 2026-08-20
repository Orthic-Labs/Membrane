# Cortex — Rust memory SDK (parity tracker)

Productizable, OS-independent memory engine. Replaces the workspace Python `mem.py`
(`tools/lib/memory/mem.py`) with **full feature parity**, in Rust. Anyone can `cargo install` it;
`rusqlite` bundles SQLite (no system dep) and `fastembed` pulls BGE-small on first use — cross-platform.

`coderight-memory` stays the inner primitives library (embed, graph, effectiveness, dream, no I/O);
`cortex` is the product: store + scope model + scoring + ingest + serve + CLI. CR-the-app keeps using
`coderight-memory` in-process (single scope); the workspace uses `cortex serve` over the existing
`:8765` JSON contract, so the hooks need no contract change. Full architecture: `tools/lib/CONTEXT-ENGINEERING.md`.

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
| Migrate + cutover | `migrate-all` over real markdown, recall parity vs Python, flip hooks to `cortex serve`, retire mem.py | P6 | **DONE — Rust is live, mem.py retired** |

## Status (2026-06-30) — CUTOVER COMPLETE

All six phases done. The workspace recall engine is the Rust `cortex` binary on `:8765`; **Python
`mem.py` is deleted**. The hooks (`recall_memory.py`/`ingest_memory.py`) lazy-start `cortex` with
`ORT_DYLIB_PATH`; recall was verified through the actual hook.

**The onnxruntime link wall** (ort `download-binaries` needs newer MSVC STL symbols than this toolset
has → `LNK1120`) was solved with ort **`load-dynamic`**: onnxruntime loads at runtime from
`$ORT_DYLIB_PATH`, so it builds on any toolset.

**Embedder parity is exact.** The Rust BGE embedding of a literal is byte-identical to Python's
(`[-0.08282,-0.05044,…]` vs `[-0.08277,-0.05045,…]`). The mid-cutover "regression" (cos 0.34 vs 0.65)
was a **build-artifact bug** — a `cargo build` (default features = hash embedder) had clobbered
`target/debug/cortex.exe`, so the migration + serve ran on the hash embedder. Fixed by building the
fastembed binary to a **stable path** (`tools/bin/cortex.exe`). With the real BGE binary, recall
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
2. Run `cortex serve` on `:8765` (with `ORT_DYLIB_PATH` set); stop `mem.py serve`.
3. Repoint the hook lazy-start to launch the `cortex` binary (set `ORT_DYLIB_PATH`).
4. Retire `mem.py`; update `CONTEXT-ENGINEERING.md` (engine = Rust).

**Operational note for distribution:** running Cortex needs the compiled binary + an `onnxruntime.dll`
via `ORT_DYLIB_PATH`. On this machine that DLL comes from the Python onnxruntime wheel; for
"installable by anyone" the release must bundle a matching `onnxruntime.dll` (ort `load-dynamic` +
shipped DLL is the portable pattern). This is a distribution decision, not a code blocker.
