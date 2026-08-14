# Deepseek — Membrane Source of Truth

**Version:** 1.0 · **Date:** 2026-08-14 · **Status:** Single canonical document
**Owner:** Adrian · **Applies to:** `Orthic-Labs/Membrane` and its workspace runtime

---

## 0. What this document is

This is the one document that defines what Membrane is, what it must become, and the exact order of work to get there. It **supersedes** every prior plan, comparison, research packet, and absorption ledger. When any other document disagrees with this one, this one wins.

**Superseded (historical only, no longer authority):**

- `membrane/sol.md`, `membrane/sol2.md`, `membrane/final_absorption.md`
- `membrane/repos/` (competitor snapshot, `sol/`, `SOL_INDEXED/`, `terra*`) — evidence only
- `membrane/research/00–03` (master synthesis, gap analysis, improvements, build plan)
- `docs/plans/membrane/**` (productization master packet, EC contracts, gap audit, state-of-truth)

The one thing this document does **not** supersede is the **executable gate tooling** — `right-ax`, `right-release`, `right-git`, `rhook`, and `Nemesis` remain the authority for *how* conformance, release, and audit are proven. This document decides *what* they must prove for Membrane.

---

## 1. What Membrane is

> **Membrane is the local-first context admission and continuity control plane that gives every agent the smallest authoritative packet it is allowed to use — and a receipt proving where the packet came from, what was omitted, what reached the host, and why.**

It is the **engine half of a two-repo system**. Cortex owns repository truth; Crypt owns durable memory; Membrane owns final admission and continuity; Sentinel owns assurance; Morph owns reviewed user-origin learning. Membrane never re-implements those.

**Membrane owns:** final admission, grants, one cross-provider attention budget, omission receipts, delivery proof, cross-client continuity, operator truth, and the packaging/install path.

**Membrane does not own:** autonomous planning/execution, the repository truth graph, deterministic SAST, or final assurance.

---

## 2. Standing constraints — non-negotiable

These cannot be traded away by any feature, competitor pattern, or deadline.

1. **First-party only. No external dependencies.** Everything Membrane needs at runtime is built, vendored, or provisioned by us: SQLite + FTS5 + local embeddings, first-party parsers and compressors, first-party sync, first-party watcher. No vector DB/platform, no hosted memory SaaS, no external MCP servers, no third-party embedding/reranker/compression service, no LSP server dependency on the hot path. Apple/Azure signing and the OS keychain are provisioned infrastructure, not runtime dependencies. The user's own model provider is the only remote call, and it is optional, bounded, cancellable, and egress-authorized.
2. **No vector platform, no Graphiti, no hosted memory.** Local embeddings in SQLite are fine; an external vector database is not. (Decision recorded 2026-07-16/07-26 and re-affirmed here.)
3. **No rerankers, multi-hop, learned compressors, or LSP/SCIP until a local measured gap proves the win.** These are evidence-gated and deferred, not silently enabled.
4. **Similarity never outranks scope, authority, current source, or explicit policy.** Authority gates run *before* ranking, never after.
5. **No silent truncation, no silent model change, no silent last-write-wins.** Every cap, timeout, dedup, fallback, and merge emits a typed reason + recovery action + completeness signal.
6. **Proposal-only learning.** Nothing auto-applies behavior changes; no auto-writes to `AGENTS.md`/`CLAUDE.md`/hooks/permissions. Human disposes.
7. **Content-free telemetry.** No prompts, paths, bodies, or secrets in canonical ledgers or exports.
8. **Measurement-class labels.** Every number is `measured | calculated | estimated | counterfactual | vendor-reported`. "Saved" is reserved for matched comparisons.
9. **Gate discipline.** Cohort/receipt machinery for anything behavior-changing; frozen runs never resumed; read-only lanes may schedule independently of mutating lanes.
10. **"Shipped" means reachable, exercised, and observably true at runtime** — implemented, consumed, activated, end-to-end tested, and measured. Structurally-complete-but-unwired code is the exact defect class this program exists to kill.

---

## 3. The substrate (what we build on, and only this)

| Layer | Fixed choice |
|---|---|
| Durable store | SQLite, WAL, `busy_timeout=5000`, `synchronous=NORMAL`, `temp_store=MEMORY`; one explicit owner per store (never "pooled" by analogy) |
| Lexical index | FTS5 (mandatory baseline; exact identifiers/errors never need a model) |
| Embeddings | Local 768-d EmbeddingGemma-class, stored in SQLite BLOBs — no ANN server, no external vector DB |
| Graph | Cheap metadata edges in SQLite tables (supersedes/contradicts/verified_by/calls); no graph platform |
| Runtime | One signed native binary (CLI + MCP + daemon + doctor + installer + updater), Rust core |
| Parsers/compressors | First-party (`skel`, `compress`, `runc`), deterministic, reversible |
| Transport | stdio MCP + loopback-only gateway (hard-fails on any non-`127.0.0.1`) |
| Identity | Opaque installation/workspace IDs; never hostname-derived; scope-chain canonicalized |
| Sync | Append-only, signed operations + content-addressed blobs; never sync SQLite/WAL files |
| Distribution | Signed native DMG (Apple Developer ID, notarized, stapled) + signed Windows EXE (Azure Artifact Signing) |

---

## 4. The target product

```text
Membrane.app / Membrane Setup.exe
├── membrane            # one signed native binary: CLI, MCP, daemon, doctor, installer, updater
├── Membrane Hub        # Tauri v2 tray + operator UI (read-only facade over delivery truth)
├── provider adapters   # Cortex, Crypt, rules, files, Git, findings, skills, anchors, compression
├── client adapters     # Claude, Codex, Cursor, Windsurf, generic MCP
├── installation manifest + identity
├── local data/config/cache/log roots
└── signed updater + rollback state
```

**Default user path:** download signed app → install → enroll repository → auto-configure clients → first context packet → host-proven delivery → receipt visible in Hub. No source checkout, Node runtime, hand-written MCP JSON, manual PID cleanup, or terminal-only diagnostics.

npm remains only as a thin bootstrapper/launcher + official MCP Registry metadata — never the product runtime. crates.io/PyPI are ecosystem seams for SDK crates, not the end-user install path.

---

## 5. The canonical gap list (what is actually wrong)

Ordered by leverage. This is the authoritative defect register; older G1–G13 / GAP-AUDIT entries are folded and corrected here.

### 5.1 Release blockers (Wave 0 — truth boundaries)

| # | Defect |
|---|---|
| B-1 | Workspace federation executes `require("node:path")` inside ESM — runtime failure on the advertised cross-repo path |
| B-2 | Workspace fan-out bypasses per-target authorization and child-grant enforcement; caller and target privilege are not intersected; no-target-match fans out to all repositories |
| B-3 | Task/turn/worktree envelopes do not survive the live path — policy and freshness cannot be proven end to end |
| B-4 | Native rule delivery is inferred from client type — a zero-byte delivery can be recorded as successful without a host-issued hash receipt |
| B-5 | Watcher liveness reimplemented from persisted PIDs — recreates the stale-PID defect Cortex already fixed |
| B-6 | Public docs and generated state drift from source (README says 6 MCP tools, source exposes 9; state manifest is commit-stale) |
| B-7 | Installer limited to Claude/Codex and writes source paths; Cursor/Windsurf manifests not productized; clean machine still needs a checkout |
| B-8 | No signed standalone release, updater, Hub, release evidence, or package channels |

### 5.2 Dead-surface reachability sweep (the systemic defect class)

The defining failure mode here is **structurally complete and semantically empty** — code that passes its own tests while being false. Confirmed instances found: three Hub projectors, a false `/health ok` over a 0-row DB, a GNU-only regex that made every Minimize check a false negative, a Mac-only release suite, a DB self-move. The systematic sweep (518+158+144+171 pub items) found four more **critical** dead surfaces, all *written-but-never-wired*:

| # | Dead surface | Action |
|---|---|---|
| S-1 | `delivery_trace_view` (Rust + Hub JS) — the delivery authority projector | **Wire it.** No Rust test exists at all. |
| S-2 | `memory_provider` (`produce_candidate_set`, LAYER 7 provider) — only test-file callers | **Wire it** into the candidate pipeline. |
| S-3 | `doc_candidate_provider` + planner shadow harness (self-disclosed "future integration") | **Wire it.** |
| S-4 | `fleet` + Hub `fleet.mjs` — installation/replication projection, both halves dead | **Wire it.** |
| S-5 | `membrane-core` crate (`budget.rs`, `fusion.rs`, `reconcile.rs`) — RRF fusion + budget authority, absent from `engine/Cargo.toml` members | **Wire it** (add to workspace members); only delete if proven abandoned. |
| S-6 | `code_batch.rs` — bounded batch admission constants | **Wire or delete** after adjudication. |
| S-7 | `mcp_http.rs` — Streamable-HTTP transport, DNS-rebinding admission, bearer auth (disclosed out-of-scope) | **Wire** behind a measured need. |
| S-8 | `notifications.rs` — alert tracker, explicitly "unrelated" in a comment | **Wire or delete.** |

The sweep is **in progress, not finished** — pub items in lib crates never trigger `dead_code` warnings. Every future "done" claim must include a reachability check.

### 5.3 Context-engine gaps (the B0–B9 spine, folded and corrected)

The validated July research + August productization converge on the same ordering. The highest-leverage gap is **not retrieval** — it is the **PUSH compression hot path** and the **token-funnel observability** around it.

| # | Gap | Truth |
|---|---|---|
| C-1 | **No PUSH plane in the hot path.** Compression engines (`runc`/`skel`/`compress`) exist but are advisory; measured adoption was 7 opportunities → 1 use. Nothing compresses what Claude Code actually accumulates (tool results, re-reads, subagent transcripts, history). | The #1 gap. Literature (AgentDiet 39.9–59.7%, CoACT 33%, CODESTRUCT 12–38%) proves the *class* of win; our own effect must come from matched local cohorts, never the vendor numbers. |
| C-2 | **Provider-token observability is built but not operationalized.** The cohort analyzer joins provider tokens with cached/non-cached separation, and `context-pulse burn` ships; what's missing is scheduling, per-task-class $, cache-break taxonomy, and live alerts. | Partially shipped; close with scheduling + attribution, not a greenfield rebuild. |
| C-3 | **The learning loop has never fired.** `context_feedback` = 0 production rows; write-time score is a constant 0.6 so the quarantine trigger cannot fire; 397/449 deliveries carried zero memory blocks. | Close delivered→used/ignored/contradicted deterministically first. |
| C-4 | **Trust is provenance-strong, authority-weak.** No authority ladder, no influence-class separation, no injection scan at intake, mirror ops unsigned, recall has no abstention state. | Harden *before* enlarging ingestion surface. |
| C-5 | **Episodic tier exists, session packets don't.** `MemoryTier::Working→Episodic→Semantic` is implemented; nothing fills it at session end. | Fill the existing tier; don't invent a new memory family. |
| C-6 | **Code intelligence is syntax + graph-lite.** No LSP/SCIP, no test↔symbol verification edges. | Verification edges first; LSP/SCIP only if a frozen gap set demands it. |
| C-7 | **No recommendation inbox.** Telemetry computes gaps with owners; nothing turns them into ranked, human-accepted proposals. | Proposal-only, decision-logged. |
| C-8 | **No recurring maintenance/analysis schedule.** Read-only analysis can run now; mutating/replication stays gated. | Separate schedules by risk. |
| C-9 | **Eval breadth.** Retrieval/replay rigor is deep; compaction-fidelity, stale-memory, poisoning, abstention, and session-resume suites are absent. | Grow the golden set from real failures. |
| C-10 | **Advisory thresholds, no enforced ceilings.** Budget guard warns but cannot act. | Advisory first; throttle only after separate approval + safety design. |

### 5.4 Packaging / interface gaps

| # | Gap |
|---|---|
| P-1 | Primary channel must be signed native, not a repo/skill/Node checkout (release blockers B-7/B-8) |
| P-2 | MCP/product protocol completeness: the documented surface (6 tools) must equal the live surface (9 tools), and both must carry full AX contract metadata (§7) |
| P-3 | Installed-path evidence: fresh install → doctor → context prepare → hash-linked packet → typed degradation on macOS + Windows |

---

## 6. Competitor absorption — what to take, and how (first-party)

This is the distilled best-parts register from the 39-repo frozen snapshot. Every entry is absorbed under the §2 constraints: first-party, local-first, no new external dependency. Items not listed here are deliberately excluded.

**Borrow directly (implement first-party):**

| Absorb | From | How it lands |
|---|---|---|
| Reversible content-addressed compression (CCR): `(compressed, savings, confidence, risk)` + recover-by-hash + explicit not-found | Supercompress, Headroom | Extends the existing `compress`/`runc` spill store; first-party, deterministic |
| Model-free streaming structural filters with Full/Degraded/Passthrough tiers | rtk | First-party stream filters over `run_capped` output; zero model calls |
| Mandatory entity/scope filter before memory retrieval | mem0 | Reinforces existing scope-chain canonicalization (C-4 hardening) |
| Temporal validity + decay/recency RRF fusion | zep, agentmemory, semantica | Retrieval-time recency/frequency decay (C-3); local only |
| Hash + watcher + `is_stale` incremental freshness; full rebuild on demand | code-compress, code-review-graph, repo-graph | Closes doc incrementality (`doc_artifacts` already stores `content_hash`+`parser_version`) |
| Cross-repo edge queries with generation + visibility intersection | codebase-memory-mcp | Federation already does this for providers; extend to cross-repo graph edges |
| Self-editing memory paging (core/recent/archival) | letta/MemGPT | Borrow the *paging pattern* into the existing episodic/semantic tiers (C-5); not a new subsystem |
| Procedural memory from violated assumptions | mengram | Feeds the recommendation inbox (C-7) as a proposal source |
| Skill Factory (crystallize lessons into skills behind a hold-out gate) | caura-memclaw, agentmemory | Proposal-only, gated; never auto-applied |
| Signed ledger + proof checker + W3C-PROV-style lineage | lean-ctx, semantica | C-4 trust hardening; first-party hash-chaining |
| WAL/checkpoint-starvation telemetry + verify-before-replace backup | codebase-memory-mcp, lean-ctx | Local-store durability ladder (§10) |
| Jailed execution + filesystem confinement + SSRF guards | context-mode, lean-ctx, synalinks | C-4 hardening; canonical-path denial |
| Typed failure taxonomy (Full/Degraded/Passthrough; classified retry) | rtk, semantica | C-10 + degradation receipts |
| Rich narrow MCP surface + thin shim for constrained hosts | agentmemory, lean-ctx, semantica | §7 + §8 |

**Explicitly rejected (do not absorb, even as patterns):**

- Unbounded pools/queues; mutable-DB file replication; opaque primary-state compression; retrying unkeyed mutations; mmap-by-default; live-SQLite replication.
- Vector-only retrieval; graph-RAG platforms (GraphRAG community summaries); cross-encoder reranking; multi-hop; learned compressors; hosted memory; external proxies as source of truth.
- Any dependency that violates §2.1 (first-party).

**Corrected facts carried forward** (fix in `sol.md` so no downstream doc repeats them):

1. **COG pool defaults** — not a single "2 + 20". Relational = `pool_size=5, max_overflow=35, pool_recycle=280, pool_timeout=280, pool_pre_ping=True`; graph postgres = `2 + 20`; vector pgvector = `2 + 20`; advisory-lock cache = `pool_size=2`.
2. **SEM languages** — 10 checked-in language packages (CodeQL, Go, Java, JSON, PHP, Python, Ruby, **Rust**, TSX, TypeScript); `Rust` must be listed, `JavaScript`/`JSX` are not separate packages.

---

## 7. Agent Experience (AX) — the conformance contract

Membrane is AX-first. Every agent-facing surface is governed by the **AXA-1 standard** (proposed internal standard) as enforced by **RightKit AX (`right-ax`)**. Membrane is **R3** in the `right-ax` rollout; until it passes, AX conformance is claimed, not proven.

### 7.1 The three result classes must stay separate (mandatory envelope)

Every operation returns:

```json
{
  "schemaVersion": "orthic.operation-result.v1",
  "operation": "membrane.context.prepare",
  "requestId": "req_01J…",
  "execution": { "status": "completed", "taskId": "…" },
  "outcome": { "status": "pass", "verdict": null, "reasonCodes": [] },
  "claimBoundary": {
    "status": "restricted",
    "cleanClaimAllowed": false,
    "safeClaims": ["…"],
    "prohibitedClaims": ["…"],
    "gaps": ["…"]
  },
  "data": {},
  "artifacts": [],
  "warnings": [],
  "nextActions": [],
  "error": null
}
```

Invocation state (`accepted|working|completed|failed|cancelled`), domain outcome (`pass|policy_fail|partial|incomplete|not_applicable|unproven`), and claim boundary (`what the agent may safely conclude`) are three distinct fields. A completed call that could not run a required provider is `outcome: incomplete`, `cleanClaimAllowed: false` — never a tool error, never a clean pass.

### 7.2 The right-ax checklist (hard gates for Membrane)

1. Small distinct tool set; no semantic-duplicate tools.
2. Wire descriptions carry when-to-use **and** when-NOT-to-use (in `tools/list`, not just docs).
3. Strict input schemas: `additionalProperties: false`, unknown fields rejected with typed errors.
4. Output schemas advertised and validated against real responses.
5. Invocation ≠ outcome ≠ claim boundary; claim boundary machine-readable (see §7.1).
6. Typed errors: stable code, `retryable` flag, remediation with next operation.
7. Bounded output: truncation receipts + continuation cursors; no silent trim.
8. Opaque handles over arbitrary paths; no model-selectable filesystem roots.
9. Effect declaration per operation (reads/writes/executes/network/destructive/idempotent/approval).
10. Long-running operations expose task semantics or document the omission.
11. No secret material in errors/logs/egress (known-secret redaction corpus).
12. Docs/examples validate against live schemas; no advertised capability without a working handler.
13. agent-plugins.org layout valid (plugin.json name constraints, skills discovery, mcp.json transport + path containment).
14. `SKILL.md` references only files that exist in the package and describes only shipped behavior.

**Gate severities for Membrane:** `static` and `conformance` are **hard** from day one; `behavioral` and `adversarial` are hard before any release. Behavioral results are **`UNPROVEN`** until produced by a real multi-agent matrix — the stub driver proves plumbing only.

**The live-surface drift (B-6) is an AX violation:** README says 6 tools, source exposes 9. Until the operation registry is the single source of truth (§8.3), this class of defect recurs.

---

## 8. Packaging — agent-plugins.org + native distribution

### 8.1 The portable plugin layer (agent-plugins.org 1.0.0)

Membrane ships the portable package at repo root, validated by `right-ax plugin validate`:

```text
membrane/
├── plugin.json                  # name, description, license; $schema → agent-plugins.org/1.0.0
├── skills/
│   └── membrane/
│       ├── SKILL.md             # Agent Skills format; every referenced path exists
│       ├── scripts/
│       └── references/
├── mcp.json                     # stdio MCP server config; $schema → agent-plugins.org/1.0.0
└── com.orthic.membrane/         # reverse-domain client extensions (hooks, per-client)
```

`plugin.json` and `mcp.json` already reference the correct `agent-plugins.org` schemas; what remains is (a) `SKILL.md` conformance (rule 14), (b) the operation-registry-single-source invariant so the manifest, MCP catalog, CLI help, SDK types, and docs never drift (B-6), and (c) `right-ax plugin validate` as a pre-release gate.

### 8.2 Distribution (the primary channel)

Portable packaging is the *discovery/manifest* layer. Distribution is **signed native**:

- macOS: Developer ID-signed, notarized, stapled DMG.
- Windows: Azure Artifact Signing Public Trust-signed setup EXE.
- Updater: signed, staged, verify-before-swap, automatic rollback, generation-fenced.
- npm: thin bootstrapper + official MCP Registry metadata only.

A capability is advertised only if its handler, schemas, authority checks, and conformance tests exist for that surface (§8.3 invariant).

### 8.3 Single source of truth for operations

One canonical operation registry drives every surface. Define each operation once (operationId, purpose, when/when-not, preconditions, input/output schema, effects+authority, execution mode, error taxonomy, examples, version/deprecation) and **generate or validate** CLI help, MCP definitions, SDK types, JSON schemas, docs, and conformance tests from it. Hand-authored duplicates are the drift defect (B-6) — eliminate them, don't patch them.

---

## 9. First-party runtime map (no external dependencies)

| Concern | Owner | Constraint |
|---|---|---|
| Hot-path hooks/policies | `tools/rhook` (Rust native) | First-party; no Python-on-hot-path for the 13 native policies |
| Durable memory | Crypt (Rust) | SQLite + FTS5 + local embeddings; no external store |
| Repository truth | Cortex | First-party |
| Embeddings | Local 768-d | No external embedding API |
| Parsers/compressors | `skel`/`compress`/`runc` | Deterministic, reversible, first-party |
| Sync | Signed append-only ops + content-addressed blobs | Never sync SQLite/WAL; first-party |
| Signing/notarization | Apple + Azure accounts | Provisioned; not a runtime dependency |
| Model calls | User's own provider | Optional, bounded, cancellable, egress-authorized |
| AX/packaging gates | `right-ax`, `right-release`, `right-git` | First-party tooling |
| Audit authority | Nemesis | First-party; `right-ax` output is an input, never a verdict |

No new external dependency may enter without an explicit product decision recorded here. This is a hard gate, not a preference.

---

## 10. The implementation order (single authoritative sequence)

Two existing roadmaps merge into one spine: productization Waves 0–4 (distribution) and the B0–B9 context-engine improvements, with `right-ax` R3 (surface conformance) woven in. Execute in order; each phase's exit evidence gates the next.

| Phase | Objective | Key work | Exit evidence |
|---|---|---|---|
| **0 · Repair truth + reachability** | Kill the false-clean defect class | B-1…B-8 (workspace auth, envelope continuity, native-delivery proof, watcher, doc truth); wire S-1…S-8 dead surfaces; add `membrane-core` to workspace members; fix telemetry-registry build boundary | No unsafe aggregate path; no dead projector reports "available"; `cargo test --workspace` green on Mac + Windows; docs == live surface |
| **1 · Token truth + PUSH default path** | See the burn, cut the burn (C-1, C-2) | Schedule read-only provider-token analyzer; per-task-class $/cache-break attribution; advisory budget ceilings; make reversible compression the **default** PostToolUse path (dedup→spill→`skel`/`compress`/`runc`→error-purge) behind a preregistered cohort | Measured packet-vs-transcript ratio replaces hypothesis; non-cached input falls without worse cache reuse/tool-calls/wall-time; raw recovery for every transform |
| **2 · Feedback + trust hardening** | Close the loop, then secure it (C-3, C-4) | Deterministic delivered→used/ignored/contradicted; fix dead 0.6 write-score trigger; retrieval-time decay; **then** authority ladder + influence class + intake scans + signed sync + abstention | `context_feedback` > 0 production rows; zero unauthorized influence in escalation/cross-scope suites; forged/replayed sync fails |
| **3 · Continuity + recommendations** | Episodic handoff, human inbox (C-5, C-7) | SessionEnd → schema-validated packet into the **existing** episodic tier; proposal-only recommendation inbox with immutable decisions | Cold session resumes a held-out task with fewer re-reads at non-inferior quality; no proposal auto-applies |
| **4 · AX + packaging conformance** | Prove the surface (B-6, B-7, P-2, §7, §8) | `right-ax static + conformance` hard-green for Membrane; single operation registry; `plugin.json`/`skills/`/`mcp.json` validated; signed native DMG + EXE, updater, Hub facade, installer for Claude/Codex/Cursor/Windsurf | `right-ax ax report` passes static+conformance+adversarial; behavioral matrix `UNPROVEN` until run; fresh install → doctor → hash-linked packet → typed degradation on macOS + Windows |
| **5 · Eval breadth + schedules** | Keep every later claim honest (C-8, C-9, C-10) | Compaction-fidelity/next-action/identifier-retention/regret suites; stale-memory + poisoning + abstention suites; session-resume; risk-separated read-only vs mutating schedules; enforce ceilings | Every real failure becomes a frozen case; every policy/component change triggers the relevant eval subset |
| **6 · Extend the lead (evidence-gated)** | Only what a measured gap justifies | Verification edges (test↔symbol) before LSP/SCIP; SDK crates; portable signed packs; encrypted team sync/fleet | Team features do not weaken local authority, privacy, or delivery proof |

**Explicitly not on the near-term path** (start only after a frozen measured gap demands it): cross-encoder rerank, LSP/SCIP, graph-RAG communities, embedding quantization, remote pooling, Streamable-HTTP (S-7), multi-hop, learned compressors, any external dependency.

---

## 11. Definition of done

A Membrane change is "done" only when all of these hold:

1. Reachable and exercised at runtime — no dead projector, no pub-item-only caller, no test-only surface.
2. Covered by the relevant `right-ax` gate and the relevant eval suite.
3. Evidence-backed: receipts, traces, or measured metrics prove the effect; estimates and vendor numbers are labeled as such.
4. Does not violate any §2 constraint (first-party, no vector platform, proposal-only, content-free, gate discipline).
5. Claim is proportionate: invocation vs outcome vs claim boundary are explicit, and the claim does not exceed the coverage.

---

## 12. Where the executable authority lives (unchanged)

| Concern | Authority |
|---|---|
| AX + packaging gates | `right-ax` (`tools/rightkit/packages/ax/`, CLI `right-ax`) |
| Release/signing | `right-release` |
| CI/commit gates | `right-git` |
| Hot-path hook policies | `rhook` |
| Audit verdicts | Nemesis |
| Durable memory / context | Crypt / Membrane engine crates |
| Repository truth | Cortex |

This document sets *what* those tools must prove for Membrane; the tools set *how* it is proven. Neither is a substitute for the other.

---

*Single source of truth v1.0. Supersedes `sol.md`, `sol2.md`, `final_absorption.md`, `research/00–03`, and `docs/plans/membrane/**`. Historical documents remain as evidence only.*
