# final_absorption.md — Absorbing the Best of 36 Competitors into Membrane

**Date:** 2026-08-12
**Reviewer note (GLM 5.2 adversarial pass, 2026-08-12):** this document was originally produced by Cline (Kimi K3) and has been adversarially cross-checked against primary vendor sources and Membrane's own state docs. Fixes applied: (1) O2 corrected — the MBR-805 adapter contract (`docs/evaluation/memory-benchmarks.md`) already accepts LoCoMo/LongMemEval/BEAM payloads, so the disposition now says "run published suites *through the existing adapter*" rather than "no suite wired"; (2) O2 now flags the mem0 LoCoMo discrepancy (mem0 self-reports 92.5; agentmemory independently measured 68.5) as the exact honesty trap the vendor-reported-until-reproduced label exists to surface; (3) closing inventory line corrected — the 10 standing rejections span §4 (7 rows) + bottom of §5 (3 rows), not "in §4"; (4) closing line now documents that M8 and G4 are double-counted (they appear both as in-table Rejects and as §4 rows 5–6 — same rejections, restated for consolidation). All other dispositions held up under verification: lean-ctx 83 tools (CHANGELOG.md), agentmemory 54 tools (ds.md) and 95.2% R@5 + ~1,900 tok/session (agentmemory README/COMPARISON.md), crg 100% blast-radius recall + 8.2× reduction (ds.md), SuperCompress "Oracle recall 100% vs truncation 24.8%" (k3.md), brain0 drift/reconcile/DLP/Ed25519 (brain0 README), schema v22 + p50 81.8 ms + 0-of-10 platform pairs (MEMBRANE-STATE.md/README), 7-opportunities/1-use Push weakness (research/01).
**Sources:** [`k3.md`](k3.md) (Function × Repo matrix, Membrane + 36 repos), [`ds.md`](ds.md) (13-axis repo comparison), [`m3.md`](m3.md) (20-repo deep matrix), [`sol.md`](sol.md) (atomic function matrix + Membrane-specific adoption order), cross-checked against current state in [`README.md`](README.md), [`docs/architecture.md`](docs/architecture.md), [`docs/MEMBRANE-STATE.md`](docs/MEMBRANE-STATE.md), and the prior research spine [`research/00–03`](research/).
**Method:** every competitor feature found in the four matrices gets exactly one disposition — **Adopt** (take it, named integration point), **Adapt** (take the idea in Membrane's idiom), **Defer** (evidence-gated, explicit trigger), or **Reject** (with reason). Nothing is absorbed by vibe; each Adopt/Adapt item names the contract it must preserve.

---

## 1. Where Membrane stands right now

**The moat (rare or absent across all 36 competitors):**

- **Typed absence accounting.** The `ContextReceipt` records what was skipped, timed out, inaccessible, or budget-dropped — not just what shipped. Across k3/ds/m3/sol, no competitor records omissions as a first-class typed artifact. lean-ctx (signed ledger + snapshots) and brain0 (tamper-evident store, DLP read audit) are the closest, and neither types *absence*.
- **Five typed contracts** (`ScopeGrant`, `ContextCandidateSet`, `ContextPacket`, `ContextReceipt`, `KnowledgeEmission`) that keep provider internals out of client adapters. Competitors leak their storage schema into their MCP tools (mem0, memclaw, agentmemory all expose store-shaped APIs).
- **Freshness/authority ranked above similarity**, root confinement, local-first data plane, four-lane budget reconciliation (`native`/`rendered`/`resolver_backed`/`metadata_only`), three process planes, dual-era MCP with enforced I/O schemas, hash-chained event log (schema v22), measured warm federation (p50 81.8 ms, resident gateway).

**The honest weakness (Membrane's own research verdict, `research/01`):** Membrane is a world-class **Pull** system with world-class accounting; **Push is built but barely fires** (`runc`/`skel`/`compress` are advisory — 7 recorded opportunities, 1 linked use), and **end-to-end token observability exists in source but nothing runs it on a schedule.** Meanwhile the dominant token burn is the session transcript Membrane doesn't yet manage.

**The competitors' center of gravity is elsewhere:** hosted memory clouds (Zep, mem0, memclaw.net), vector-DB sprawl (mem0's 20+ adapters), agent frameworks (Letta, PraisonAI, LangChain), and tool-count races (lean-ctx 83 tools, agentmemory 54). That is where they spend; it is not where Membrane should follow.

## 2. The absorption thesis

Four tests decide every disposition. A feature is absorbed only if it:

1. **Strengthens a motion Membrane already owns** (Push, Pull, Persist) or one of its two cross-cutting planes (Observe, Govern). Membrane does not absorb new product categories.
2. **Deepens the moat** — receipts, typed contracts, freshness discipline, local-first. Features that make the receipt *more* truthful jump the queue.
3. **Can be gated and measured** under existing discipline: frozen invariants in CI, preregistered contracts, labeled evidence classes (measured / calculated / estimated / vendor-reported).
4. **Costs less than the failure it removes.** The field's most-copied features (LLM extraction on every write, graph construction per repo, cross-encoder rerankers) are also its largest ingest taxes (sol.md §Performance readout: GR, COG, M0, MCL, MEMY carry the heaviest ingest costs). Membrane absorbs work-avoidance designs first.

**Corollary — what absorption is *not*:** not a tool-count race (lean-ctx's 83 tools vs. Membrane's deliberate 10), not a storage migration (SQLite+FTS5+in-process f32 vectors is settled), not a business model (no hosted tier, no fleet), not an agent runtime.

---

## 3. Absorption inventory by motion

### 3.1 PUSH — make compression fire instead of advise

The highest-leverage lane: Membrane already owns the engines, and the field proves the default-on pattern works.

| # | Feature & best source | What they do | Membrane today | Disposition & move |
|---|---|---|---|---|
| P1 | **Default-on interception proxy** — rtk | PreToolUse hook filters 100+ shell commands before output enters context; "smart" mode strips bodies to signatures; tee saves originals | `runc` exists, instrumented, reversible — but advisory, so ~0 adoption | **Adopt.** Wire `runc` into the PostToolUse hot path with a per-command filter registry (Git + test runners first, per sol.md's adapter bound). Gate: preregistered cohort contract, raw recovery for every transformed item. This *is* research B3 — rtk's contribution is proof that default-on, not advisory, is the whole game. |
| P2 | **Streaming capture, spill-on-breach** — rtk + sol.md | Stream output; begin spill only after cap breach; never buffer full output in memory | `run_capped` preserves head/tail + exit status but captures full output and always spills | **Adopt.** Complete streaming `run_capped` exactly as sol.md's correction table specifies. |
| P3 | **Reversible compression, content-addressed restore (CCR)** — Headroom, SuperCompress | Dropped blocks replaced by hash markers (`[SC-Retrieve: hash]`); originals restorable verbatim | `runc` spill dir is already content-addressed; reversibility not unified across the trio | **Adopt.** Promote the spill dir to the CCR store for all three Push tools; one marker format, one restore path, receipt records the marker↔original binding. SuperCompress's eval framing ("oracle recall 100% vs truncation 24.8%") becomes the fidelity suite (feeds B9). |
| P4 | **Query-aware compression** — Headroom, SuperCompress | Compress *against the current query*; keep entities/errors/definitions/dependencies | Push tools are query-blind | **Adapt.** Pass the active task/intent (already a `membrane_context` input) into Push routing so kept spans are task-biased. Measure against a fixed corpus first (sol.md FTS5-row discipline). |
| P5 | **Pipeline lifecycle event taxonomy** — Headroom | One canonical event spine (Setup→Pre/Post-Start→Input Cached/Routed/Compressed→Pre/Post-Send) that hooks, middleware, telemetry all hang off | Hook events exist (SessionStart, memory-rearm, pre/post-compact checkpoints); no single named lifecycle | **Adapt.** Name Membrane's lifecycle once (assemble→admit→render→deliver→reconcile), emit it on every packet, let receipts and the value ledger hang off it. Small, pure clarity win. |
| P6 | **Execute-don't-load sandbox** — context-mode (`ctx_execute`) | Agent runs sandboxed code against data instead of loading it; claims 98% context savings | Nothing | **Defer.** Radical and real, but a new trust surface (sandboxing, exfil guard) plus a new tool family. Trigger: after P1–P3 land, if measured transcript burn still dominates and a frozen task set shows execution beats retrieval on it. If adopted, it enters as one bounded tool under root confinement, never a second product. |

### 3.2 PULL — deepen ranking honesty and lexical muscle

| # | Feature & best source | What they do | Membrane today | Disposition & move |
|---|---|---|---|---|
| R1 | **FTS5 + identifier-aware tokenization** — cbm, crg, context-mode, lean-ctx | FTS5 BM25 with camelCase/code tokenizers; exact identifiers dominate coding work | No checked-in FTS5 retriever path (sol.md correction row) | **Adopt, gated.** Add behind a fixed corpus, deterministic fallback, and measured benefit — exactly sol.md's precondition. cbm's camelCase tokenizer is the specific detail to steal: exact-match identifiers are already Membrane doctrine. |
| R2 | **Reciprocal Rank Fusion + query-kind boosting** — crg, context8 | Merge BM25 + vector ranks via RRF; boost by query kind (symbol vs error vs prose) | Reserved-lane admission (memory 800 / skills 300) + freshness/authority ranking | **Adapt.** RRF runs *within* a provider's candidate set; the cross-provider lane policy stays (raw provider scores are not cross-comparable — settled in MEMBRANE-STATE). Query-kind boosting maps onto the existing `intent` input. |
| R3 | **Blind-spot notes on structural answers** — repo-graph (`orient`) | The code map says what it *cannot see* alongside what it returns | Receipts record absence for federation; Cortex structural answers don't emit blind spots | **Adopt.** Extend `membrane_cortex` results (architecture/impact/references) with a typed blind-spot field (unindexed languages, stale generation, truncated depth). Receipts-for-absence applied to the code graph — pure moat-deepening. |
| R4 | **Per-candidate score explanation** — mem0 "explain mode" | Every retrieved item carries why-it-ranked | Receipt explains admission/skips globally; per-candidate provenance thinner | **Adopt.** Stamp each admitted candidate with its winning signal (freshness rank, lane, lexical/semantic score class) in the receipt. Turns "why did the agent know X" into the same lookup as "why didn't it." |
| R5 | **Bounded provider-result cache** — sol.md LRU row | Reuse warm provider results across near-identical prompts | No bounded LRU found | **Defer-then-measure.** Only with explicit cache key, invalidation key, capacity, eviction, and hit/miss evidence, per sol.md. Trigger: warm-federation profiling shows repeated identical provider calls. |
| R6 | **Zero-copy mmap index** — repo-graph (rkyv `.gmap`), cbm | Cold structural queries in milliseconds without a DB | Cortex snapshots exist; cold-start cost unmeasured in the matrices | **Defer.** Trigger: measured cold-start pain. Not before. |
| R7 | **Cross-encoder rerank / dynamic-K / multi-hop / graph expansion** — mem0, SuperCompress, memclaw, cognee | Rerank fused candidates with a cross-encoder; expand via KG hops | — | **Reject for now.** All four sit on the research spine's evidence-gated deferral list (02 §6); the matrices add no new evidence to reopen them. Revisit only via a frozen retrieval-gap set proving material gain. |

### 3.3 PERSIST — automate the memory lifecycle Crypt already types

| # | Feature & best source | What they do | Membrane today | Disposition & move |
|---|---|---|---|---|
| M1 | **Keystones — pinned per-session critical memories** — memclaw | Operator/agent pins a small set that always loads, per session | Reserved memory lane (800 tokens) exists; no explicit pin primitive | **Adopt.** Add a `pinned` class inside the existing reserved memory lane — typed, scoped, expiry-bound, receipt-visible. Fits the lane policy without new budget machinery. |
| M2 | **4-tier consolidation pipeline** — agentmemory | Working→Episodic→Semantic→Procedural with Ebbinghaus decay, contradiction detection, citation provenance | `MemoryTier::Episodic` exists but unfilled; decay/dedup/supersession exist; consolidation not pipelined | **Adapt.** This is B7 with a named pipeline. SessionEnd → archive-first episodic packet (goal, decisions, open work, dead ends, verification, identifiers, revision, lineage) → promotion to semantic/procedural stays a *separate reviewed action*. Decay constants from measured recall curves, not defaults. |
| M3 | **Procedural memory evolving from violated assumptions** — mengram | Workflows update when reality breaks them; violations feed revision | KnowledgeEmission proposals exist; no procedural family | **Adapt.** Add a procedural emission type whose mutations are always proposals (human-gated, standing doctrine). mengram's violated-assumption trigger becomes one proposal source in the B8 inbox. |
| M4 | **"The Interviewer" — memory mined from agent trails** — memclaw | Offline pass synthesizes session transcripts into candidate memories | Knowledge proposals are agent-initiated; no transcript miner | **Adapt.** A read-only offline miner over the already-captured transcript census, emitting KnowledgeEmission *proposals* into quarantine. Never auto-writes. The B8 recommendation inbox's richest feed. |
| M5 | **Contradiction detection + supersession** — memclaw, agentmemory, lean-ctx | Detect conflicting claims; close validity; keep both with a supersession edge | Temporal supersession with single-valued predicate policy exists; detection manual | **Adopt.** Automate *detection* (deterministic first: same subject+predicate, differing value, overlapping validity); keep the settled resolution — keep both, close validity, supersession edge, never tombstone-only (research §2 resolved this). |
| M6 | **Session replay/snapshot + PreCompact capture** — agentmemory, cline, context-mode | Replayable session state; snapshots fire on the compact boundary | Pre/post-compact checkpoints + `membrane_checkpoint_save/load` exist | **Aligned — extend lightly.** Add the PreCompact hook event (context-mode's pattern) so checkpoint capture fires on the compact boundary automatically, not opportunistically. |
| M7 | **Memory importers** (ChatGPT export, Obsidian) — mengram | Bulk-import external corpora as typed memories | None | **Defer.** Real value, zero urgency until M2–M4 prove the lifecycle. Trigger: first external-corpus request, then one importer behind the same quarantine review. |
| M8 | **LLM enrichment on every write** — mem0, memclaw, cognee | Each write pays an LLM pass for facts/tags/relations | Deterministic writes | **Reject.** Heaviest ingest tax in the field (sol.md readout); the write path stays deterministic, LLM judgment confined to offline, sampled, human-labeled calibration (B5/B8). |


### 3.4 GOVERN — harden before volume grows

| # | Feature & best source | What they do | Membrane today | Disposition & move |
|---|---|---|---|---|
| G1 | **Privacy/secret filter before any LLM touch** — agentmemory (dedup→privacy filter→LLM compress), context-mode (hard-block exfil) | Nothing leaves or gets summarized before a scan; curl/wget exfil hard-blocked at hook | Root confinement + local-first plane; no pre-compress scan in the Push path | **Adopt.** Deterministic secret/injection scan at `crypt put` and at the Push transform boundary — exactly research B6's write-gate scan; agentmemory's pipeline proves the ordering (filter *before* compress, never after). |
| G2 | **Signed savings/evidence receipts** — lean-ctx (ed25519 ledger), brain0 (tamper-evident store), rtk (integrity verify) | Cryptographically signed claims of what was saved/done | Hash-chained event log (schema v22) shipped; Ed25519 mirror signing planned | **Adopt.** Complete the planned Ed25519 work and extend signing to the *savings receipts* the value ledger emits — a claimed token saving becomes as verifiable as a claimed write. Skip lean-ctx's Lean 4 proof checker (ceremony > value; a signed receipt is the right-sized proof). |
| G3 | **Drift detection — declared vs done** — brain0 | Compare what the agent said it would do against what actually changed | Audit provider exists; no declared-vs-done comparator | **Adapt.** Feed plan/task envelopes (MBR-007 identity already flows end-to-end) and the Git provider's change stream into a drift check; surface via the audit lane and receipt. High moat value: a truthfulness receipt for agent behavior, not just context. |
| G4 | **Per-tenant trust tiers / fleet RBAC** — memclaw, vanna | Multi-tenant isolation, row-level security, trust tiers | Single-operator; scope grants | **Reject.** Fleet/team is an undecided commercial boundary (README pricing block). ScopeGrant already provides the single-operator version of the same invariant. |
| G5 | **Canonical prefix enforcement before any I/O** — sol.md correction row | Validate owning boundary before filesystem/DB/subprocess work; typed failures | Partial | **Adopt.** Direct from sol.md's table — validate canonical prefixes before fs/db/subprocess, return typed failures. Cheap, closes a real hole. |

### 3.5 OBSERVE — become the field's most honest measurement surface

| # | Feature & best source | What they do | Membrane today | Disposition & move |
|---|---|---|---|---|
| O1 | **Per-command savings ledger** — rtk `gain` | Running tally of tokens not sent, per command, verifiable | Context-value ledger + telemetry exist; savings not surfaced per-transform | **Adopt.** Emit a per-transform savings receipt (before/after counts, restore hash) into the ledger; aggregate in the daily report (B1). Numbers carry evidence-class labels per standing rule. |
| O2 | **Public benchmark harness as a second evidence lane** — mem0 (LoCoMo 92.5, self-reported), agentmemory (95.2% R@5 LongMemEval-S @ ~1,900 tok/session), crg (100% blast-radius recall, 8.2× token reduction), lean-ctx eval/, headroom benchmarks/ | Published, rerunnable suites with named numbers | Golden-set eval culture exists; the MBR-805 adapter contract (`docs/evaluation/memory-benchmarks.md`) already accepts LoCoMo/LongMemEval/BEAM payloads — but no published suite has been run through it | **Adapt.** Wire LoCoMo/LongMemEval suites *through the existing MBR-805 adapter contract* alongside the golden set, labeled vendor-reported-until-reproduced. Adds the one thing the field respects that Membrane lacks: comparable numbers. Blast-radius recall becomes the Cortex impact-query metric (crg's 100%-recall bar). Note the honesty trap: agentmemory independently measured mem0 at **68.5%** on LoCoMo vs mem0's self-reported 92.5 — exactly the vendor-reported-until-reproduced gap this lane exists to surface. |
| O3 | **Cache-break forensics** — lean-ctx (prompt-cache-safe proxy), Codex-loop exact-prefix discipline | Classify every cache-ratio drop by cause (model switch, tool-list churn, index size) | B4 planned | **Adopt (already planned).** The matrices confirm the cause taxonomy; lean-ctx's cache-safe pinning validates B4's classification list. No scope change. |
| O4 | **Doctor / perf self-diagnosis** — headroom (`doctor`, `perf`), lean-ctx (dashboard); cbm's unmeasured "sub-ms" as the cautionary tale | One command that proves health and speed with numbers | `docs/doctor.md`, e2e-benchmark harness exist | **Aligned — extend lightly.** Add a perf subcommand that re-runs the warm-federation measurement and prints it beside the published p50/p95. cbm's unmeasured claim is what Membrane must never do. |

### 3.6 DELIVERY & REACH

| # | Feature & best source | What they do | Membrane today | Disposition & move |
|---|---|---|---|---|
| D1 | **Full hook-event coverage** — agentmemory (12 CC events incl. PostToolUseFailure, TaskCompleted), context-mode (PreCompact), cline (veto) | Fire on every lifecycle edge the host exposes | SessionStart, memory-rearm, PostToolUse (planned Push), checkpoints | **Adapt.** Add PreCompact + PostToolUseFailure + TaskCompleted/SessionEnd capture; skip tool_call veto (that's Arcane/rhook's layer in the parent workspace, not Membrane's). |
| D2 | **17-platform adapter coverage** — context-mode, lean-ctx (30+ agents) | Per-client hook/plugin manifests everywhere | 7 host adapters with honest capability levels | **Defer (rightly).** Coverage follows the support-matrix receipts (0 of 10 pairs qualified today). Grow adapters only through the receipt-gated qualification path — never claim a platform without the conformance receipt. |
| D3 | **Single-binary / zero-dep distribution** — rtk, lean-ctx, cbm | One static binary; nothing to provision | npm package + Rust engine + loopback service | **Aligned.** Membrane's floor is already local, no mandatory services. Keep the install story; don't chase single-binary purity at the cost of the three-plane architecture. |
| D4 | **Auto-generated repo wiki / docs** — context8, crg | LLM-written wiki regenerated from the graph | KnowledgeEmission covers durable facts | **Defer.** A wiki is a rendering of candidates Membrane already types; build only if a frozen eval shows agents consume wiki-form better than packets. |

---

## 4. What we deliberately do NOT absorb

| Rejected feature | Sources | Why |
|---|---|---|
| Hosted/cloud memory plane | Zep Cloud, mem0 cloud, memclaw.net, SuperCompress Firebase | Violates the local-first data plane — the README's first promise. |
| Vector-DB platform sprawl | mem0's 20+ adapters, Qdrant/pgvector defaults everywhere | Settled: SQLite+FTS5 baseline, vectors in Crypt, resident in-process f32 dispatch (bake-off, commit `4bd2f9d9`). |
| Agent runtime / framework | Letta, PraisonAI, LangChain, bondai, haystack agents | Membrane is the context control plane *under* agents, not an agent. Absorbing a runtime dissolves the moat. |
| Tool-count race | lean-ctx 83, agentmemory 54 MCP tools | Membrane's 10-tool surface with enforced schemas is a feature. Each absorbed capability above maps onto an existing tool, not a new one. |
| LLM-on-every-write extraction | mem0 v3, memclaw enrichment, cognee cognify | The field's largest ingest tax (sol.md readout). Offline, human-gated proposals only (M3/M4). |
| Multi-tenant fleet / RBAC | memclaw, vanna, context8 | Undecided commercial boundary; ScopeGrant covers the operator case. |
| RDF/ontology spine | memonto, cognee grounding | Typed contracts already give the structure without SPARQL's weight. |

## 5. Sequenced absorption roadmap

This roadmap **overlays** the two standing plans rather than competing with them: the research spine's B0–B9 build plan (`research/03`) and sol.md's engineering adoption order. Every absorbed feature names which plan item it joins. One clock, gates between phases.

**Phase 0 — Freeze the shape (sol.md final-shape rule, before anything else).**
Freeze in CI: packet contents/order, typed degradation, grants, freshness/generation identity, cancellation, no-op sync, command output/exit behavior, warm federation receipts. Every later phase must preserve these outputs while reducing measured work. *Joins: B0.*

**Phase 1 — Push activation (the burn cut).** P1 default-on interception → P2 streaming `run_capped` → P3 CCR unification → G1 pre-compress filter → G5 prefix enforcement → P4 query-aware routing (measured) → P5 lifecycle spine. *Joins: B3, B6's write-gate.* Exit: cohort contract met (40% reduction / five-point quality margin), raw recovery 100%, savings receipts flowing (O1).

**Phase 2 — See everything.** O1 savings ledger surfaced in the daily report → O3 cache-break forensics → O4 perf self-diagnosis → O2 external benchmark lane. *Joins: B1, B2, B4, B9.* Exit: tokens-per-successful-task and cache-hit ratio reported daily without being asked; one labeled public-suite number reproduced.

**Phase 3 — Pull deepening (evidence-gated).** R1 FTS5 + camelCase behind a fixed corpus → R2 RRF + query-kind boosting → R3 blind-spot notes → R4 per-candidate explanation. R5/R6 only if profiling demands. *Joins: sol.md FTS5/LRU rows.* Exit: measured retrieval win on the frozen corpus with deterministic fallback intact.

**Phase 4 — Persist lifecycle.** M1 keystones lane → M2 episodic consolidation (SessionEnd packets) → M5 contradiction detection → M6 PreCompact capture → M3 procedural proposals → M4 Interviewer miner. *Joins: B7, then B8.* Exit: a cold session resumes a held-out task with fewer re-reads at non-inferior quality; first mined proposal accepted through quarantine.

**Phase 5 — Govern to depth.** G2 signed savings receipts → G3 drift detection. *Joins: B6 remainder.* Exit: forged/replayed sync ops fail; every published saving is signature-verifiable; first drift finding surfaced on a real session.

**Phase 6 — Reach, only with receipts.** D1 hook-event completion → D2 adapter growth strictly through the support-matrix qualification path. Exit: every claimed platform pair carries a conformance receipt.

**Standing deferrals (do not start; triggers named above):** P6 execute-sandbox, R5 LRU, R6 mmap, M7 importers, D4 wiki. **Standing rejections:** §4, all ten rows.

| Lean 4 proof checker | lean-ctx | Signed receipts (G2) deliver the verifiability at a fraction of the machinery. |
| Conversation-history compaction ownership | every host | Explicitly out of scope per README posture — stays with each host; Membrane feeds it checkpoints (M6) instead. |
| Event-sourced dialogue state | Rasa | Different domain (dialogue tracking, maintenance mode). |

---


## 6. How we know absorption worked

| Signal | Measured by | Target shape |
|---|---|---|
| Push actually fires | Transform-invocation rate vs. opportunity ledger (B1 census) | From 1-of-7 to default-on for capped classes; zero unrecoverable transforms |
| Burn falls | Non-cached input tokens per successful task, daily report | Preregistered cohort contract, quality margin held |
| Claims stay honest | % published numbers carrying evidence-class labels + signatures (G2) | 100%; zero cbm-style unmeasured claims |
| Memory compounds | `context_feedback` production rows, compounding curve computable | Curve exists and bends the right way (B5/B8) |
| Receipts answer new questions | Blind-spot and per-candidate fields in use (R3/R4) | "Why didn't it know X" and "why did it rank Y" both one lookup |
| Nothing breaks | Phase-0 frozen fixtures green on every merge | Zero tolerated regressions |

## 7. Standing rules for every absorbed feature

1. Every published number labeled: measured / calculated / estimated / counterfactual / vendor-reported.
2. One preregistered contract per behavior-changing experiment; never blend contracts after results arrive.
3. Proposal-only learning: nothing auto-applies, nothing auto-writes to CLAUDE.md/AGENTS.md/hooks.
4. Content-free telemetry contract unchanged.
5. Each phase, when scheduled, gets its own bounded contract (files/lines/minutes ceilings) per workspace plan discipline — the sizes here are sequencing classes, not estimates.
6. Locked invariants hold everywhere: typed contracts, provider authority vs freshness distinct, omissions in receipts, local/loopback/repo-confined, fresh code outranks stale memory, degraded state reported — never a false clean.

---

*End of final_absorption.md. 34 inventory dispositions: 25 Adopt/Adapt/Aligned-extend, 6 Defer with named triggers, 3 in-table Rejects (R7, M8, G4) — plus 10 standing rejections consolidated across §4 (7 rows) and the bottom of §5 (3 rows: Lean 4 proof checker, conversation-history compaction, event-sourced dialogue state). Note: M8 and G4 appear both as in-table Rejects (§3.3/§3.4) and in the §4 standing list (rows 5–6) — they are the same rejections restated for consolidation, not separate items. Sources: k3.md, ds.md, m3.md, sol.md; reconciled with research/00–03 and docs/MEMBRANE-STATE.md. Cross-checked 2026-08-12 against primary vendor sources (agentmemory README/COMPARISON.md, mem0 repo, lean-ctx CHANGELOG, brain0 README) and Membrane's own MBR-805 benchmark contract.*
