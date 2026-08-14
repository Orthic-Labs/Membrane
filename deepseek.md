> Superseded by `solimplement.md` on 2026-08-14.
> Retained as historical context; do not treat as current.

# deepseek.md — historical Cortex implementation draft

Status: **historical draft.** Current implementation authority is `solimplement.md`. This draft supplied AX sequencing & competitor-mechanism proposals; corrected dispositions are recorded there.

---

## 1. What "best" means (two axes, one gate)

Cortex is "best" only when **both** axes are proven, not assumed:

| Axis | Definition | Where proven |
|---|---|---|
| **A — Correctness/performance** | Noninferior on every eligible axis (coverage, exactness, freshness, latency, CPU/RSS/disk, privacy, explainability, recovery, lifecycle, portability, surface) + strictly better on ≥1 material axis. | `sol.md` comparative gate + frozen benchmark protocol |
| **B — Agent Experience (AX)** | End-to-end probability that an unfamiliar agent accomplishes the intended goal correctly, safely, recoverably, with warranted claims. | AXA-1 conformance + behavioral harness (`pass^k`) |

Axis B is the *currently unproven* half. Axis A is mostly specified but not fully implemented. A fast backend with poor AX is not "best" — the AX standard is explicit that "a tool is not judged efficient merely because its backend is fast."

---

## 2. Binding constraints (non-negotiable; no workstream may weaken them)

1. **Local-only, zero egress** — repository content, chunks, symbols, claims, embeddings, and prompts never leave the machine (`sol.md` CX-I08/CX-I22).
2. **Vector DB is written off** — no mandatory ANN, no embeddings, no hosted backend, no multi-store architecture. SQLite is the sole authority (CX-F75). Nothing reopens this.
3. **First-party only, no new dependencies** — every mechanism is a Node stdlib builtin (`node:sqlite`, `node:zlib`, `node:crypto`, `node:fs`) or a first-party reimplementation. No native addons, no new npm packages, no vendored C.
4. **MCP is read-only** — every MCP operation is read-only; proposals are sealed artifacts, never source/repository effects (CX-I17).
5. **Hub-owned lifecycle** — Hub alone controls start/stop/restart; Cortex exits on ownership loss; no independent daemon (CX-I11).
6. **Evidence before claims** — no false clean, no empty-success, no unsupported completion claim (CX-I19/CX-F97).
7. **Node core frozen** — Node core, parser ownership, CLI names, MCP governance, and Tier A/B/C labels are frozen boundaries (CX-I20).

---

## 3. The complete workstream map (one dependency-ordered sequence)

Every work item from the three absorbed plans, deduplicated and ordered. Each milestone lists its source, what it delivers, and its exit gate.

| # | Milestone | Delivers | Source | Exit gate |
|---|---|---|---|---|
| M0 | Baseline & protocol freeze | Frozen corpus/holdouts, benchmark manifests, competitor manifest, capacity manifest, dirty-tree classification | P0 | Test harness green against frozen expected behavior |
| M1 | Truth & coverage | U0–U5 universal disposition (100% of files), capability lattice, one provider registry, fact schema | P1–P3 | `U1..U5 + U0 = 100%`; no module/test-only capability claims |
| M2 | Resolution & snapshot soundness | Indexed global resolution qualified, ghost-edge equivalence, `BuildSnapshotV1` atomic adoption, true no-op fast path | P4 | Ghost-edge suite passes; no-op `<1 s`; 550-file cold `<5 s`/`<300 MB` |
| M3 | Lifecycle & concurrency | Daemon build singleflight, cancellation/waiter isolation, Hub fencing, exact resident routing | P5 | Concurrent build = one build; interrupted publication preserves prior generation; Hub-off census 0 |
| M4 | Exact search & storage write path | One exact authority (BM25 + symbol bypass), IDF token columns, direct-page SQLite writes, dual codec (deflate+brotli), fused decompression+Aho-Corasick scan | P6 + competitor §4.1/4.2/5.1/5.2 | exact p95 `<5 ms`; delta `<100 ms`; direct-page byte-equals SQL path; no new dependency |
| M5 | AX contracts (agent surface) | `mcp serve` onboarding, resources/prompts registered, typed `outputSchema`/`structuredContent`, annotations, effect profiles, `claimBoundary`, actionable error remediation | W1 + W2 | Fresh `init` → working MCP handshake; every tool schema-valid + claim-bounded |
| M6 | Precise providers & depth | Compiler/LSP/SCIP adapters, custom grammars, framework/schema/IaC facts, dependency policies | P7–P8 | Compiler-tier fixtures per language; unsupported dimensions typed |
| M7 | Hybrid retrieval & ranking | Truth-ranked candidate map (centrality/verification/semantic components visible), hard token ceiling, typed abstention, deterministic fallback | P9 + competitor §9 | Budget never exceeded; semantic cannot alter authority; held-out recall ≥ frozen baseline |
| M8 | Behavioral AX harness | AXA-1 conformance runner (CI), 12 behavioral scenarios, `pass^k` (`^1/^3/^5`), routing confusion matrix, no-tool + claim-fidelity metrics | W6 + AX standard | Conformance exit 0 in CI; first `pass^3` report committed; overclaim rate measured |
| M9 | Security & atomicity | Handle-bound atomic rename (Windows/POSIX), build-snapshot manifest (grammar/dirty-hash fields), envelope encryption + crypto-shred (`node:crypto`), redaction/tamper evidence | Competitor §6–8 + CX-F125 | Crash fixtures leave no torn file; encryption round-trips opaque; secrets redacted |
| M10 | Release & distribution | Update trust root (pinned Ed25519), native receipt gate, WinGet publish, Python compiler tier (scip-python) | W3–W5 | Signed-manifest round-trip; valid receipts pass + absent receipts fail; Python exact refs COMPILER-tier |
| M11 | Cross-repo & recovery | Federation (one generation per repo), backup/restore/repair, corrupt-DB recovery, compatibility matrix, comparative benchmark run | P10 | Every CX-R requirement green; Mac + Windows receipts |
| M12 | Memory/zero-copy (last, gated) | JS-idiomatic arena/zero-copy spans, mmap read-only export | Competitor §8 | Only after CX-F77 gates: `≥20%` material win, same facts/results |

**Ordering rule:** M0→M1→M2→M3→M4→M5→M6→M7→M8→M9→M10→M11→M12. Truth/exactness precedes AX contracts precedes storage precedes security precedes release. M8 (behavioral AX) is *not* last — it is the highest-value unproven surface and lands as soon as M5's typed surface exists. M12 is gated behind measured bottleneck proof.

---

## 4. Competitor mechanism ledger (what to adopt, gate, or reject)

Complete disposition of every borrowed mechanism, with first-party reality. Source files are in `/Volumes/D/claude/cortex/repos/**`.

### 4.1 Storage write path

| Mechanism | Source (file) | Disposition | First-party reality |
|---|---|---|---|
| Direct B-tree page construction (no SQL INSERTs) | `codebase-memory-mcp/internal/cbm/sqlite_writer.c` | **ADOPT** | Write our own builder against `node:sqlite`'s page format; build-only; byte-equal to SQL path |
| Streaming mid-pipeline flush (free heavy `properties` as written) | `codebase-memory-mcp/internal/cbm/sqlite_writer.h` | **ADOPT** | Same discipline in the JS loader |
| Dual codec selection | `codebase-memory-mcp/{lz4_store,zstd_store}.c` | **ADAPT** | LZ4/zstd are native addons → **not adoptable**. Use `node:zlib` deflate (latency) + brotli (size). First-party, zero new deps. |
| IDF-weighted token column for BM25 | `codebase-memory-mcp/internal/cbm/sqlite_writer.h` (`CBMDumpTokenVec.idf`) | **ADOPT** | One indexed column; IDF version in provider/version identity |
| int8-quantized embedding vector columns | `codebase-memory-mcp/internal/cbm/sqlite_writer.h` (`CBMDumpVector`) | **REJECT** | Vector bake-off. No vector column, no ANN, no second store. |

### 4.2 Exact search

| Mechanism | Source | Disposition | First-party reality |
|---|---|---|---|
| Fused decompression + Aho-Corasick scan (search compressed in place) | `codebase-memory-mcp/internal/cbm/ac.c` | **ADOPT** | AC automaton = first-party JS; decompression = `node:zlib`. No dependency. |
| FTS5 BM25 + symbol bypass | Roam / code-review-graph | **ALREADY + ADOPT** | Consolidate into one measured exact authority |
| Bitmask pattern-match result (one word per N patterns) | `codebase-memory-mcp/internal/cbm/ac.c` | **ADOPT** | BigInt bitmask in JS |

### 4.3 At-rest security

| Mechanism | Source | Disposition | First-party reality |
|---|---|---|---|
| Application-layer envelope encryption (wrapped DEK, KEK rotation, crypto-shred) | `Brain0/crates/brain0-storage/src/payload.rs` | **ADOPT** (gated CX-F125) | AES-256-GCM via `node:crypto` builtin; no external crypto package |
| Redaction + tamper evidence as output-surface concern | Brain0 `redact.rs`/`secret.rs`, Roam HMAC attestations | **ALREADY** (CX-F117/CX-F42) | Node builtins |

### 4.4 Atomicity & identity

| Mechanism | Source | Disposition | First-party reality |
|---|---|---|---|
| Handle-bound atomic rename: `renameat2(RENAME_NOREPLACE)`, `renameatx_np(RENAME_EXCL)`, Windows `SetFileInformationByHandle(FileRenameInfo)` | `Roam/src/roam/atomic_io.py` | **ADOPT** | `node:fs` + first-party native binding via the existing `node:fs` primitives; no dependency |
| Build-snapshot manifest: version + schema + parser/grammar versions + config hash + git HEAD + dirty hash + edge `bridge_version` | `Roam/src/roam/index/manifest.py` | **ADOPT** | Extend `BuildSnapshotV1` (already exists) |

### 4.5 Pools

| Mechanism | Source | Disposition | First-party reality |
|---|---|---|---|
| Bounded thread-safe per-language parser pool (`Queue(maxsize)`, reset-before-reuse) | `treesitter-chunker/chunker/_internal/factory.py` | **ALREADY + ADOPT** (measure) | First-party; not a worker pool (worker-pool gate unaffected) |
| SQL pool with `pool_pre_ping`/`pool_recycle`/`pool_timeout`/`NullPool` default | `cognee/.../SqlAlchemyAdapter.py` | **ADOPT** (name knobs in CX-F118) | `node:sqlite` handle discipline, first-party |
| Single resource-handle pool lifecycle (lease/pin/TTL/eviction/shutdown) | treesitter-chunker + cognee + CBM | **ADOPT** (consolidate) | CX-F123 |

### 4.6 Ranking & budgeting

| Mechanism | Source | Disposition | First-party reality |
|---|---|---|---|
| PageRank-style graph centrality + hard token ceiling | Aider repo-map (per `sol.md` Appendix A) | **ADOPT** | First-party; centrality is a ranking signal, never authority |

### 4.7 Memory / zero-copy (gated M12)

| Mechanism | Source | Disposition | First-party reality |
|---|---|---|---|
| JS-idiomatic arena/zero-copy spans | Oxc arena, repo-graph rkyv (per `sol.md` Appendix A) | **GATE** (CX-F77) | `Buffer`/`ArrayBuffer` slices; no Rust rewrite |
| mmap read-only compact export | repo-graph `.gmap` | **GATE** (CX-F77) | First-party native via Node |

---

## 5. AX requirements (condensed from the AX standard)

The standard's 24 principles reduce to these concrete Cortex requirements. Result-class separation and claim fidelity are **hard gates**, not optimization metrics.

### 5.1 Result-class separation (mandatory on every operation)

Every result returns three distinct layers, never conflated:

```text
invocation  : accepted | working | completed | failed | cancelled
outcome     : pass | policy_fail | partial | incomplete | not_applicable | unproven
claim       : what the agent may safely conclude (safeClaims / prohibitedClaims / gaps)
```

Violations to ban outright: success-by-exit-0, empty-means-clean, clean-claim-with-missing-evidence, domain-failure-as-crash.

### 5.2 The compact surface (already specified in `sol.md` CX-F44/F71)

Default agent surface = exactly eight intent-level read operations, progressively disclosed:

```text
orient · context · search · impact · verify · truth · proof · status
```

Advanced capabilities appear only through schema-versioned discover/expand. This is the AX standard's "small, distinct, progressively disclosed" principle made concrete.

### 5.3 Contract requirements per operation

- Strict typed input (`additionalProperties: false`, enums, server-side validation).
- Typed output (`outputSchema` + `structuredContent`; text is secondary rendering).
- Effect profile (reads/writes/executes/network/installs/destructive/idempotent).
- Actionable errors (stable code, retryability, remediation, next operation, state preserved).
- Opaque handles, not raw filesystem paths, for authority arguments.
- Idempotency keys for mutating operations; preview/commit separation.

### 5.4 Behavioral requirements (the currently-unproven half)

| Requirement | Measure | Target |
|---|---|---|
| Routing correctness | first-operation accuracy + routing confusion matrix | no systematic `audit↔review`, `doctor↔audit`, `search↔fetch` confusion |
| No-tool decisions | no-tool accuracy | agent correctly refrains when nothing applies |
| Argument validity | first-attempt validity rate | minimal retries, no invented fields |
| Recovery | recovery rate from induced failures | recover without unsafe workarounds |
| Repeated-run reliability | `pass^k` (report `^1/^3/^5`) | stable across trials, not mean-only |
| Cross-agent reliability | multi-agent/multi-model matrix | reported variance |
| Claim fidelity | overclaim_rate, clean-claim false-positive rate | safety-critical false-positive = hard release failure |
| Final-state verification | environment state, not prose | completion judged from durable state |

### 5.5 Required behavioral scenarios (frozen starting denominator)

From the AX standard §8.2 (retrieval/context systems) + W6's 12:

1. Fuzzy routing (no tool name given).
2. Correct no-tool decision.
3. Stale-repo claim test (must not claim "current").
4. Missing-argument recovery.
5. Generation-mismatch recovery.
6. Budget/truncation behavior.
7. Root-escape attempt (containment).
8. Injection inside repository docs.
9. Multi-tool `orient → expand → impact`.
10. Pagination continuation.
11. Error-remediation follow-up.
12. Claim fidelity on stale generation.

Plus the retrieval-profile required cases: ambiguous query, stale index, no-result, conflicting sources, malicious instructions in retrieved content, large-corpus narrowing.

---

## 6. Single acceptance matrix (all gates in one place)

| Axis | Gate | Milestone |
|---|---|---|
| Coverage | `U1..U5 + U0 = 100%`; unexplained files/bytes `0` | M1 |
| Truthfulness | Capability cells from production receipts; no module/test-only claims | M1 |
| Correctness | Ordered exact results, ghost edges, provider/generation identity equivalent | M2 |
| Snapshot | One `BuildSnapshotV1`; source/provider/resolver churn exposes `0` mixed rows | M2 |
| Cold build | 550 files `<5 s`, `<300 MB RSS`; 5,000 files `<60 s`, `<1 GB RSS` | M2/M4 |
| Incremental | no-op `<1 s`; one-file delta `<100 ms`; 100-file update `≥10×` | M4 |
| Query | resident exact p95 `<5 ms`; deterministic under concurrency | M4 |
| Storage | Direct-page byte-equals SQL; no new dependency; old-generation residue bounded | M4 |
| AX contract | Every tool schema-valid; annotations + effects + claimBoundary present | M5 |
| AX behavioral | Conformance exit 0; first `pass^3`; overclaim rate below threshold | M8 |
| Security | No critical/high unauthorized effect; secrets redacted; injection can't expand privilege | M9 |
| Recovery | Prior generation survives interruption/corruption; migrations rebuild/rollback | M9/M11 |
| Lifecycle | One fenced Hub owner; Hub-off census `0`; no independent persistence | M3 |
| Portability | Identical source/provider/schema contract on Mac + Windows | M11 |
| Compatibility | CLI/MCP/daemon/SDK/schema supported-version matrix `100%` | M11 |
| Comparative | Frozen manifest; all-axis noninferiority + one material dominance, or "target" wording only | M11 |
| Dependencies | `package.json` dependency count unchanged from today's baseline | all |

---

## 7. Mapping to `sol.md`

This plan adds to — never weakens — `sol.md`. Where it touches a frozen requirement, it extends it:

| This plan | Extends `sol.md` |
|---|---|
| M1–M3 | CX-R001–R016 |
| M4 (direct-page, dual codec, IDF, fused AC) | CX-F122, CX-F126, CX-I08, CX-A13 |
| M5 (typed outputs, claimBoundary, effects) | CX-F05/F71, CX-F37, CX-I17 |
| M6 | CX-R021–R024, CX-A04/A05/A16/A24 |
| M7 (ranking, centrality, budget) | CX-F07/F09/F38/F39 |
| M8 (behavioral AX) | CX-F115 (independent verification), AXA-1 |
| M9 (rename, manifest, encryption) | CX-F145/F146, CX-I13, CX-F125 |
| M10 | CX-F63 (SCIP), CX-I15 (surfaces), release gates |
| M11 | CX-R029–R036 |
| M12 | CX-F77 (native-layout gate) |

Vector-DB/ANN bake-off, first-party-only, local-only, read-only MCP, and Hub lifecycle remain the binding constraints of §2 and are unchanged from `sol.md`.

---

## 8. What is deliberately NOT in this plan

- **No vector store, no ANN, no embeddings** — baked off.
- **No external dependency** — every mechanism is first-party (Node builtins or self-written).
- **No source mutation / codemod surface** in Cortex core.
- **No independent daemon** — Hub owns lifecycle.
- **No Rust rewrite / Node SEA** — deferred with named gates (`sol.md` CX-F77 / release decisions).
- **No hosted/team mode, no plugin marketplace** — deferred.

---

## 9. Provenance & honesty

- Every competitor mechanism cites a file inspected in `/Volumes/D/claude/cortex/repos/**` or a claim in `sol.md` Appendix A. Appendix-A-only claims (Oxc arena, repo-graph rkyv, Aider PageRank) are marked and require independent re-verification before promotion (CX-F115/CX-I23).
- The two most under-captured competitors in `sol.md` are **codebase-memory-mcp** (direct-page writes, fused LZ4/AC, zstd, int8/IDF columns) and **Brain0** (envelope encryption + crypto-shred); §4.1–4.3 are the direct consequence.
- Axis B (AX) remains formally **UNPROVEN** until M8's behavioral harness executes real-agent runs. This plan states that plainly; it does not claim AX.
- "Best" is a target, not a present claim, until M11's comparative gate and M8's behavioral gate both pass.
