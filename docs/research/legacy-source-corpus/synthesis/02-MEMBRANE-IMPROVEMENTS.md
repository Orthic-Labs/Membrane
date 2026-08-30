# Membrane Improvements — Validated Corrections and Priorities

**Date:** 2026-07-26  
**Scope:** Validation of the four source reports, [`00-MASTER-SYNTHESIS.md`](00-MASTER-SYNTHESIS.md), [`01-MEMBRANE-GAP-ANALYSIS.md`](01-MEMBRANE-GAP-ANALYSIS.md), and the current repository evidence needed to assess the gaps.  
**Change boundary:** Research document only. No application code, configuration, runtime, database, scheduler, or existing research file was changed.

## 1. Executive verdict

Fable's central conclusion survives review: Membrane is strongest in scoped retrieval, provenance, accounting, replay discipline, and cross-machine operation, while its largest remaining product opportunity is managing the context that accumulates around that retrieval lane.

The two documents are not fully correct as written:

- The master synthesis is **substantively reliable but not a complete or mechanically auditable consolidation**. It captures nearly all production-relevant themes, and independent primary-source checks confirmed its important benchmark figures. It still omits several source points, over-credits Perplexity for two stronger claims, and labels internal design targets as “validated.”
- The gap analysis is **directionally strong but operationally stale in two material places**. Provider-token analysis already exists in current source/docs, and Cortex already implements an episodic memory tier. The actual gaps are operational coverage/scheduling and automatic session-packet capture, respectively.
- The roadmap remains useful after those corrections, but its order should change: establish one current truth boundary, operationalize the observability already built, make PUSH measurable, close the feedback loop, harden trust before expanding persistent ingestion, then add session packets and recommendations.

## 2. Validation of the master synthesis

### 2.1 What is covered well

The synthesis faithfully consolidates the four reports' main production architecture:

- PUSH, PULL, PERSIST, ASSEMBLE, OBSERVE/EVALUATE/IMPROVE, and GOVERN/PROTECT;
- typed provenance, scope isolation, immutable evidence, versioned claims, contradiction handling, and human-approved policy change;
- reversible content-aware compaction, cache-stable assembly, hybrid retrieval, dynamic budgeting, and abstention;
- code-aware retrieval, lifecycle curation, telemetry, matched replay, security, harness adapters, and multi-machine operation;
- the principal disagreements on contradiction retention, authority gating, local storage, Headroom adoption, graph scope, and compression order.

Independent checks against primary sources confirmed the load-bearing figures used in the synthesis and gap analysis:

- [AgentDiet](https://arxiv.org/abs/2509.23586): 39.9–59.7% input-token reduction and 21.1–35.9% total-cost reduction at maintained agent performance.
- [CoACT](https://arxiv.org/abs/2607.02911): 33.0% average total-token reduction with task effectiveness close to the uncompressed agent.
- [CODESTRUCT](https://aclanthology.org/2026.acl-long.607/): 12–38% token reduction for most evaluated models and 1.2–5.0% Pass@1 improvement on SWE-bench Verified.
- [Mem0](https://arxiv.org/abs/2504.19413): 26% relative LLM-judge improvement, 91% lower p95 latency, and more than 90% token-cost savings against its full-context comparison.
- [Tool-schema compression](https://arxiv.org/abs/2605.26165): 44–50% schema savings and a 20.5 percentage-point exact-match lift in the reported 8K overflow setting.
- [Codebase-Memory](https://arxiv.org/abs/2603.27277): 83% versus 92% answer quality at ten times fewer tokens and 2.1 times fewer tool calls.
- [OpenAI's Codex loop](https://openai.com/index/unrolling-the-codex-agent-loop/): exact-prefix caching, cache breaks from mutable configuration/tool lists, and the nondeterministic MCP tool-order bug.
- [Claude prompt caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching): five-minute default TTL, 1.25× five-minute writes, 2× one-hour writes, and 0.1× cache reads.
- [OWASP Agent Memory Guard](https://owasp.org/www-project-agent-memory-guard/) and ASI06: persistent memory is explicitly treated as a poisoning surface.
- [Headroom](https://github.com/headroomlabs-ai/headroom): the 60–95% JSON and 15–20% coding-agent reductions are project-reported claims, so Fable was right not to treat them as independent proof.

These checks validate the direction and the quoted benchmark values. They do **not** prove that Membrane will reproduce those savings; each result has its own agent, workload, model, and intervention.

### 2.2 Corrections and omissions

| Finding | Evidence | Correction |
|---|---|---|
| The citation ledger is incomplete. | Sol cites **Mnemis** at `solCONTEXT_MEMORY_KERNEL_IMPLEMENTATION_PLAN_2026-07-24.md:4606`; it does not appear in the master ledger. The paper exists and describes dual-route retrieval over base and hierarchical graphs: [ACL 2026.1096](https://aclanthology.org/2026.acl-long.1096/). | Add Mnemis to the ledger or stop calling the ledger complete. |
| Several M3 frontier items disappear. | `m3CONTEXT-MANAGEMENT-SYSTEM.md:2808-2813` names policy-learned memory management, causally grounded retrieval, multimodal procedural memory, native memory-efficient architectures, benchmark standardization, and sleep-style consolidation. The master captures the last two only partially and omits the first four as explicit frontier items. | Preserve them in a “deferred research questions” section; do not promote them into the near-term build. |
| Perplexity is over-credited for physical index separation. | Master point 17 gives P a checkmark. P defines distinct object types and storage technologies (`prplxcontext-memory-system.md:124-195`) but does not require a separate physical index per memory type; M does (`m3CONTEXT-MANAGEMENT-SYSTEM.md:235`, `2621`). | Mark P as partial/implicit, not full agreement. |
| Perplexity is over-credited for a full context manifest. | Master point 23 equates P's `package_hash` with a manifest of every included and omitted item, position, tokens, reasons, hashes, and cache key. P records included memory IDs/package hash and call cost (`prplxcontext-memory-system.md:365-373`), not the complete omission receipt. | Mark P partial. |
| “Validated operating targets” mixes evidence with product choices. | The ≤1 pp margin, ≥20% reduction, 60–90% cache ratio, retrieval thresholds, and ≥100-query set in §4.3 are design targets from the reports, not all empirical facts. | Rename the table “proposed operating targets” and label each row `external result`, `design target`, or `current product gate`. |
| The synthesis is difficult to re-audit mechanically. | Most inventory rows cite only S/Q/P/M, not exact sections or source lines. | Add per-row source sections or a machine-readable claim ledger before using “complete.” |

### 2.3 Master-synthesis verdict

Use the master synthesis as the architectural summary and research index. Do not use its “complete,” “all checked,” or “validated target” labels as proof without the corrections above.

## 3. Validation of the Membrane gap analysis

### 3.1 Gap-by-gap disposition

| Gap | Disposition | Validation |
|---|---|---|
| G1 — PUSH absent from the hot path | **Verified** | `runc`, `skel`, and `compress` are real and reversible, but the installed opportunity ledger recorded seven recommendations and one linked use (`docs/MEMBRANE-STATE.md:31-37`). The system measures advice more reliably than it executes it. |
| G2 — provider-token/cost/cache blindness | **Corrected: partial, not absent** | Current state explicitly says provider usage is joined to hook delivery and quality data, with cached and non-cached input separated (`docs/MEMBRANE-STATE.md:871-892`). The dashboard reads `provider_tokens` and displays non-cached/cached totals (`engine/crates/cortex/src/dashboard.html:308-325`). The remaining gap is freshness, coverage, scheduling, cost attribution, and live operator alerts. |
| G3 — no full per-call context manifest | **Verified with harness limitation** | Membrane has detailed packet receipts, but hooks cannot observe the final provider prompt. Post-hoc reconstruction must remain labeled approximate. |
| G4 — learning loop has not fired | **Verified** | The deployed feature table says the feedback rail is live but unexercised, with zero production rows (`docs/MEMBRANE-STATE.md:478`); the later audit repeats the all-zero result (`docs/MEMBRANE-STATE.md:510-517`). |
| G5 — no evidence-utilization closure | **Verified** | The schema can record used/ignored/contradicted, but production feedback is empty. Delivery evidence currently stops before reliable value attribution. |
| G6 — no structured recommendation inbox | **Verified** | The repository exposes diagnostics and typed gaps but no evidence-backed proposal lifecycle, accept/reject history, or taste calibration. |
| G7 — “nothing runs on a schedule” | **Corrected wording** | Runtime services and hooks operate, but recurring daily analysis/curation/replication remains deliberately disabled (`docs/MEMBRANE-STATE.md:41`, `82-89`). Say “no recurring maintenance/analysis schedule,” not “nothing.” |
| G8 — authority and influence controls are weak | **Verified** | The memory schema records tier, provenance-like metadata, scope, producer, and record type, but no authority level or influence class (`engine/crates/cortex/src/memdb.rs:11-30`). The state doc still lists typed authority/lifecycle as open (`docs/MEMBRANE-STATE.md:500`). |
| G9 — no episodic layer/session packets/handoffs | **Corrected: tier exists; capture does not** | `MemoryTier` already implements `Working → Episodic → Semantic` (`engine/crates/cortex-core/src/types.rs:5-21`), and the focused promotion test passes. What is absent is an automatic session-end packet, lineage-bound handoff, and policy separating session episodes from durable semantic claims. |
| G10 — code intelligence is partial | **Verified at the capability level; rebaseline required** | The material gap—no proven LSP/SCIP semantic layer or test↔symbol verification graph—survives. The precise description of the Blueprint provider is time-sensitive and must be regenerated from the current Blueprint manifest rather than copied from older state docs. |
| G11 — advanced retrieval refinements deferred | **Verified** | Deferring rerankers, HyDE, multi-hop, and learned selection until local evals show a gap is consistent with all four reports. |
| G12 — eval breadth | **Verified** | Existing retrieval/replay rigor does not replace compaction-fidelity, stale-memory, poisoning, abstention, and end-task outcome suites. |
| G13 — no live budget guard | **Verified** | Provider-token analysis and a dashboard goal exist, but there is no demonstrated live daily/session quota guard with warn/throttle behavior. |

### 3.2 Claims that must be downgraded

- The diagram's `~10⁵–10⁶ billed tokens` and `~90%+ of burn` are not backed by a recorded Membrane measurement in the cited evidence. Treat them as a plausible hypothesis until provider-session analysis reports the actual distribution.
- “Tens to hundreds of times larger” is likewise an inference, not a measured ratio.
- The 33–60% external benchmark range is evidence that PUSH is worth testing, not Membrane's expected effect. Membrane's effect must come from matched local cohorts.
- The current system already has a configured provider-token experiment with a 40% goal and a five-point quality non-inferiority margin (`docs/MEMBRANE-STATE.md:882-892`), while the master proposes ≥20% and ≤1 pp. These are different product contracts. Pick one preregistered contract per experiment; do not blend them after results arrive.

### 3.3 Reproducibility issue surfaced during validation

The focused `cortex-core` episodic-promotion test passed. The focused Cortex analysis-endpoint test could not compile in this checkout because `engine/crates/cortex/src/context_telemetry.rs:130` embeds `lib/context-telemetry-registry.json`, which is absent here.

This may be a sparse-checkout packaging boundary rather than a runtime defect, but it means this checkout cannot independently reproduce the analysis-endpoint proof. A supported source checkout should either include the registry or declare and verify the external prerequisite before build.

## 4. Corrected improvement register

| Priority | Improvement | Origin | What should change | Acceptance evidence |
|---|---|---|---|---|
| P0 | **Canonical current-state and experiment contract** | Audit refinement | Separate `installed/live`, `source-only`, `historical`, and `planned` claims. Reconcile the 20% vs 40% reduction goals and 1 pp vs five-point quality margins before another efficiency claim. Restore a self-contained supported build or an explicit prerequisite check for the telemetry registry. | One dated manifest binds source, installed generation, analyzer version, provider/session coverage, control, reduction target, quality margin, and rollback. Focused analysis tests compile from a supported checkout. |
| P0 | **Operationalize the provider-token analysis already built** | Fable R0, corrected | Do not build a second “Token Observatory.” Finish and schedule the existing analyzer; expose report age, matched/unmatched sessions, input/output/cache-read/cache-write, model/client/day/session attribution, and unknown coverage. Keep cached and non-cached economics separate. | Fresh report arrives without manual invocation; unavailable sources remain unknown; content-free export tests pass; every published ratio carries a denominator and coverage rate. |
| P0 | **Live budget guard** | Fable G13 | Add configurable warn/critical ceilings over the same provider-token ledger. Start advisory; throttle only after a separate approval and safety design. | Deterministic replay proves threshold crossings, reset windows, model pricing/version handling, and no false “zero” when usage is unavailable. |
| P1 | **Default-path reversible PUSH** | Fable R1 | Gate the lowest-loss ladder: exact dedupe → archive/externalize → typed reduction → extractive/abstractive compression only when needed. Reuse `runc`, `skel`, `compress`, spill storage, identity, and cohort machinery. Do not begin with learned free-text compression. | Raw recovery succeeds for every transformed item; identifier/error/failing-test preservation passes; matched cohorts satisfy the preregistered quality margin; non-cached input improves without worse cache reuse, tool calls, or wall time. |
| P1 | **Cache-break diagnostics** | Fable R2 | Classify model, tool-list/order, permission, cwd, serializer, static-rule, and prefix changes using real provider usage. Treat this as diagnosis, not an assumed savings number. | Every material cache-ratio drop has a bounded reason or `unknown`; changing the diagnostic itself cannot mutate the prompt. |
| P2 | **Close delivered→used/ignored/contradicted** | Fable R3 | Start with deterministic references to delivered IDs/paths/symbols; record ambiguous cases as `unknown`. Add sampled calibrated judging only after human labels exist. Use verified outcomes for veto/promotion; never promote a weak proxy into truth. | Production feedback is non-zero; precision is measured on a labeled sample; false-use and cross-scope tests pass; utility signals can be replayed without changing the original delivery record. |
| P2 | **Trust hardening before new ingestion volume** | Fable R6, reordered | Add origin-derived authority and influence classes, injection/secret scanning, provenance-based quarantine, signed mirror operations, and an explicit abstention result. Apply this before automatic session packets enlarge the persistent attack surface. | Cross-scope and instruction-escalation suites return zero unauthorized influence; forged/replayed sync operations fail; quarantined data cannot become instruction; deletion/tombstone behavior is reversible and tested. |
| P3 | **Session packets using the existing episodic tier** | Fable R5, corrected | Add archive-first, schema-validated session-close packets containing goal, decisions, open work, failures/dead ends, verification, exact identifiers, repo revision, lineage, and raw references. Promotion to semantic/procedural memory remains a separate reviewed action. | Packet generation fails closed; a cold session can resume a held-out task with fewer re-reads at non-inferior quality; contradictory/open state is not flattened; session packets expire or demote by policy. |
| P3 | **Evidence-backed recommendation inbox** | Fable R4 | Generate proposals only after token, cache, feedback, and session evidence are reliable. Each proposal needs traces, current/proposed diff, expected metric, risk, eval, rollback, and a human decision. | No proposal auto-applies; accepted/rejected/deferred decisions are immutable and versioned; replay shows whether accepted proposals helped. |
| P4 | **Eval expansion** | Fable R7 | Add compaction fidelity, next-action preservation, identifier/error retention, regret/refetch, stale-memory, poisoning, abstention, session-resume, and full-task outcome suites. Grow from real failures. | Each new production failure becomes a frozen case; every policy/component change triggers the relevant subset; efficiency is reported only after the quality gate. |
| P4 | **Separate schedules by risk** | Fable R8 | Read-only analysis can run independently. Replication, curation, and policy-affecting jobs retain separate names, locks, receipts, rollback, and promotion gates. | A failed analyzer cannot mutate memory or enable another scheduler; missed/stale runs fail loudly; concurrent runs do not duplicate work. |
| Deferred | **Verification edges before LSP/SCIP** | Fable G10 | First prove test/failure↔symbol and change↔symbol edges from existing deterministic evidence. Add LSP/SCIP only if a frozen retrieval-gap set shows material gain over current Blueprint/exact search. | Provider qualification reports capability by language/edge kind; local task outcomes—not graph size—justify promotion. |

## 5. Recommended order

1. **Truth first:** P0 current-state contract, reproducible analysis build, existing provider-token analyzer, and advisory budget alerts.
2. **Cut measured waste:** P1 reversible PUSH and cache-break diagnostics behind the current cohort/replay machinery.
3. **Make value observable and safe:** P2 feedback closure, then trust/authority hardening.
4. **Add continuity:** P3 episodic session packets and handoffs, followed by the recommendation inbox.
5. **Broaden proof:** P4 eval coverage and risk-separated scheduling.
6. **Keep retrieval expansion evidence-gated:** verification edges first; rerankers, LSP/SCIP, multi-hop, learned compressors, and new stores remain deferred.

This order differs from Fable's roadmap in three deliberate ways:

- R0 becomes an operationalization of existing provider-token analysis, not a greenfield observatory.
- Trust hardening moves before automatic session ingestion.
- Session packets reuse the existing episodic tier instead of introducing another memory family.

## 6. Preserve these decisions

- Keep SQLite/FTS/local embeddings and append-only sync; do not add a vector platform or hosted memory dependency without a measured scaling need.
- Keep packet receipts, immutable failed runs, scope isolation, content-free exports, and human approval gates.
- Keep exact/lexical search first-class for identifiers and errors.
- Keep advanced retrieval and learned compression behind local matched evaluation.
- Never represent estimates, external benchmark results, or vendor claims as measured Membrane savings.

## 7. Final disposition

The research supports improving Membrane, not replacing it. The highest-value work is to finish the whole-context measurement and execution loop around the strong retrieval/accounting core already present:

`provider truth → reversible PUSH → utilization feedback → authority hardening → episodic handoff → human-approved recommendations`.

That is the smallest path from today's system to the architecture the four reports actually converge on.
