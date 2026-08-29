# Membrane Subsystem Audit — 2026-08-29

**Method:** seven parallel code-verified reviews (one per subsystem plus runtime/protocol), each
bound to the repo's own three-fact ethos — implementation presence / production reachability /
qualification evidence — with absence claims allowed only as "not found by search".
**Lineage baseline:** the four archived context-engineering documents (frozen snapshot 2026-07-16:
`CONTEXT-ENGINEERING.md`, `UNIFIED-CONTEXT-SYSTEM-ARCHITECTURE.md`, `RIGHTCONTEXT-STATE.md`,
`CONTEXT-ENGINEERING-EVOLUTION.md`).
**Status:** point-in-time audit snapshot, not canon. Re-derive from current `main` before acting.

---

## The ledger

| Subsystem | Verdict | Headline evidence |
|---|---|---|
| Hub / Runtime / Protocol | **● strong** | Architecture B (tray → OS-coupled daemon) is landed ahead of its own docs; five V1 shapes have cross-language golden-fixture discipline. But the CI lifecycle gate greps stale doc prose and passes green against code it contradicts. |
| Cortex | **● strong core** | Admission sealing, scope-chain soft bonus, SHA-aware veto, fail-loud embedder all real and production-wired. Missing: relevance spot-check harness, file-level backup/restore, any cross-machine story. |
| Adapt | **● strong core** | 33 deterministic detector families with adversarial-negative tests, wired into the live CLI/resident path. Evidence layer honestly empty: no real held-out corpus; 0.95 precision gate has no automatic feed. |
| Blueprint | **▲ solid, honesty regressed** | Generations/freshness/watcher/lease core is production-grade. Coverage disposition accounting and the six-state doc-claim truth model regressed vs. the predecessor design; no graph import path. |
| Pull | **▲ solid, unqualified** | All nine predecessor provider lanes survive and are reachable. Fusion has receipts but no comparative qualification; corrective retrieval is implemented but dormant (no production caller supplies a sufficiency contract). |
| Push | **◆ mis-wired** | Transforms, protected spans, and restore are solid — but the live `/federate` path bypasses the classify→transform ladder and query-aware policy, compressing code like prose, with no production telemetry. |
| Ledger | **◆ dark in production** | The most mature indexing engineering and the best eval corpus in the repo (152 cases, never run). Live retrieval sits behind two off-switches: an empty FTS receipt allowlist and a default-off provider flag. |

> **Overall:** the architecture is genuinely top-class in shape — typed receipts, provenance,
> admission gates, and honesty invariants exceed most shipping systems. No subsystem is at its
> optimum yet, and the gap is one theme everywhere: **mechanisms are built and honest, but the
> evaluation and wiring loops that would qualify them have not been run.** Built but not fed;
> wired but not qualified.

---

## Subsystem detail

### Hub / Runtime / Protocol — strong

The tray→headless-daemon topology with OS-enforced lifetime coupling (Windows Job Object; macOS
kqueue) is implemented, and the Hub app itself provably carries no resident runtime (its own test
asserts no `run_hub_runtime`, tray builder, or thread spawn in production source).
`hub_inactive` is a real health probe, not a stub; MCP/CLI clients are genuinely stateless (they
only POST to loopback, never spawn). All five public V1 shapes (`ScopeGrantV1`,
`ContextCandidateSetV1`, `ContextPacketV1`, `ContextReceiptV1`, `KnowledgeEmissionV1`) carry
hand-pinned schemas, golden fixtures, and pinned canonical digests enforced identically in Rust and
JS. Seventeen MCP tools are live. Skills-as-context **survived** — a provenance-sealed
`SkillsProvider` plus `cortex skill-read` mirrors the predecessor pattern.

**The defect that matters:** `scripts/ci/check-lifecycle-conformance.mjs` runs in every CI job but
asserts the *retired* in-process topology by grepping doc prose — a live, CI-blessed false-green
gate. `docs/operations/resident-lifecycle.md` and `docs/hub-handoff.md` are stale the same way.
And `scripts/qualification/install-release.ps1` — the only real installed-artifact qualification —
is not invoked by any workflow, so none of the ten Appendix A lifecycle scenarios has gated
installed-artifact coverage.

**Fix:** point the conformance gate at code, not prose; refresh the two stale docs; wire
`install-release.ps1` into release CI.

### Cortex — strong core

Healthier than a docs-only read suggests. Admission is sealed and replay-guarded
(`memory_batch_receipt`/`item_sha256`); retrieval is hybrid FTS5+vector with RRF candidate
generation; the predecessor's hard-won mechanics survive verbatim — scope-chain soft bonus
(`SCOPE_CHAIN_SOFT_BONUS = 0.02`/rank with the soft-not-hard-sort test), sibling isolation,
asymmetric query/document embedding prompts, fail-loud embedder init (hash fallback only behind
`MEMBRANE_ALLOW_HASH=1`), inject/access counters wired from the live recall/fetch paths, and the
SHA-aware contradicted-veto in the shared recall path (stale vetoes drop when content hash
changes). Bounded one-hop wikilink recall (0.3 discount) is live. Deterministic Dream with
reversible quarantine works; Stage 1 semantic curation is proposal-only and correctly unwired.

**Gaps:** the 20-query blind relevance spot-check exists only as a code comment; no file-level
backup/restore (content export + row-restore only); exact-duplicate pruning deletes directly
instead of routing through quarantine; the semantic floor is looser than the predecessor's
(`SEMANTIC_THRESHOLD=0.30` OR'd with lexical match); no cross-machine replication of any kind; the
whole-doc-vs-chunking decision survived but its frozen tournament evidence did not;
`membrane_temporal_fact` is schema-only with no wired insert path; `cortex-core::MemoryGraph` is
implemented but unreachable.

**Fix:** rebuild the relevance spot-check harness; add backup/restore with integrity verification;
route duplicate pruning through quarantine.

### Adapt — strong core

~16.8k lines of native Rust, genuinely wired into `membrane adapt
mine/review/review-taste/adjudicate-taste/apply/apply-insights/report/benchmark/doctor/context-cost`
and the resident Taste-delivery path (`serve.rs` calls `select_delivery_candidates` directly).
33 deterministic detector families with real false-positive guards (clause-local negation, quoted
text, hypotheticals, tool-relayed text; UTF-8-boundary tests). Sealing, recurrence
(`FailureEpisodeV1`→`InsightIssueV1`, min 2, state matrix), remediation with orthogonal
`RemediationEffect`×`InterventionTarget`, and the 0.95 precision gate are all implemented and
tested. The transcript substrate parses ten hosts (Claude Code, Codex, CommandCode, Cline, Qwen,
Pi, Gemini, Grok Build, Roo-Cline, plus OpenCode/Cursor discovery) with byte-precise span binding.

**Gaps:** both corpora are synthetic and say so — no real held-out corpus and no measured
production precision exist anywhere; `seal_actionable`'s precision gate has no automatic feed from
a real scorecard (callable, not auto-invoked outside tests); effectiveness ledgers have no live
telemetry writer; emergent discovery and the evaluator lifecycle are absent by search; no
model-assisted detector is actually constructed (the enum variant exists unused).

**Fix:** build the real held-out corpus (the benchmark harness already exists); wire
`family_precision` to real scorecards; implement or explicitly defer the emergent-discovery lane.

### Blueprint — solid, honesty regressed

The generation/freshness/watcher core is production-grade: content-hashed immutable generations
(xxh128 + sha256 manifest digest) with atomic adoption, four typed freshness states including
`changed_since_generation`, labeled dirty/live overlays, a Hub-authorized watcher
(`MEMBRANE_HUB_CHILD=1` + PPID + launch token) with real OS advisory locks (`BEGIN EXCLUSIVE`) and
tested crash recovery. Incremental refresh does changed-file-only reparse with first-class rename
events, backed by real equivalence tests in blueprint's own CI. Tree-sitter covers 36 languages at
AST tier (SCIP optional, python-only); doc↔code joins are built into every generation and
queryable via CLI/HTTP/MCP. Query surface (`search/resolve/recall/expand/impact/path/...`) is
bounded (`MAX_HOPS=2`, token budgets, typed `budgetOmissions`) with evidence/confidence on results.

**Regressions vs. the predecessor's honesty guarantees:** the complete-coverage disposition
taxonomy is not implemented — `scanSources()` silently skips oversized/unreadable/binary files with
no counter; doctor reports no coverage percentage (the canon's "93.2%" is illustrative, not live).
The doc-claim truth model collapsed from six states to three (`supports/contradicts/supersedes`),
classified by regex over claim text; a comment-claims extractor hardcodes `status: "implemented"`.
Portability is a one-way freshness manifest — `graph export` is a debug dump and **no
`graph import` exists at all**. Blueprint's qualification CI is not invoked by the parent repo's
top-level CI.

**Fix:** per-file disposition records + a real coverage percentage in doctor, gated in CI; typed
six-state claim verdicts; a graph import path with a round-trip test.

### Pull — solid, unqualified

The registry hard-fails unless all nine provider lanes are present — `anchors, architect, audit,
blueprint, cortex, git, live_files, rules, skills` — each with an implementation, a contract test,
and wiring into the real `pull federate` CLI/resident path. So nothing from the predecessor's
nine-provider set quietly disappeared (Ledger is correctly absent: navigation, not evidence).
Fusion emits versioned strategy receipts (`membrane-fusion-fixed-v1` default,
`membrane-fusion-rrf-v1` opt-in) with per-candidate decisions. Typed omission accounting
(`ProviderOmissionV1` → `packet.omissions`) reaches the packet. Sufficiency evaluation
(`SufficiencyContractV1` → per-requirement Satisfied/Missing/Unavailable) and a one-stage
alternate-lane corrective pass (never repeats the trigger lane) are implemented and unit-tested.

**Gaps:** no relevance-labeled corpus or task-success metric backs the fusion comparison —
`docs/evidence/qualification/pull-metrics.json` honestly says `mechanics-qualified-no-promotion`,
held-out n=1 run once, latency/cost `not_instrumented`; corrective retrieval is dormant — no
first-party caller supplies a sufficiency contract, so live requests report
`not_evaluated_missing_sufficiency_contract`; no cross-provider starvation protection (the
predecessor's reserved lanes: memory 800 / skill 300, then global fill); and the predecessor's
measured lesson that within-provider fused-RRF ordering degraded ranking (mean rank 2.37→6.07,
reverted) is preserved only by architectural accident — Cortex ranks internally, federation fuses
across lanes — not as a stated, tested qualification principle.

**Fix:** wire a first-party sufficiency caller so corrective retrieval executes end-to-end; build
the labeled qualification corpus with the within/cross-provider distinction explicit; decide
reserved-lane floors deliberately (implement or document the omission).

### Push — mis-wired

The mechanism layer is well built: the full predecessor transform set survives (`skel` tree-sitter
skeletons for Rust/Python/JS/TS, structure-safe `compress` with path:line/fence/link protection,
`runc` head/tail capping with digest-verified spill and restore via `/expand`, prep routing:
missing/tiny→copy, structured→exact copy, code→skel, large markdown→outline, prose→compress).
Protected-span verification and `PacketReductionPlanV1` selection (largest-fit, floor,
estimator-basis mismatch refusal) are rigorously tested. Restore fails closed on
missing/expired/unavailable.

**The wiring gap:** the live `/federate` packet-reduction path
(`push::selection::select_packet_for_h8`) calls one generic `compress_to_budget_with_options` on
every non-protected block — code is compressed like prose; the classify→transform ladder and the
explicit `PushPolicy::QueryAware` exist only on the manually invoked CLI `prep` path; and
production reduction emits no telemetry (`push::telemetry::record` is CLI-only and env-gated).
The one place users actually receive reduced context never uses the subsystem's intelligence.
Qualification: `push-metrics.json` is `mechanics-qualified-no-promotion` — retention/protected-span
metrics on 4 frozen cases; task-correctness/latency/restores `unavailable`.

**Fix:** route `reduced_1` through the same classify→transform dispatch as `prep`; thread
planner-supplied query/task metadata into that production call; emit telemetry from it.

### Ledger — dark in production

Structurally the most mature retrieval engineering in the repo: Comrak/GFM AST projection with
source positions and typed block coverage, weighted FTS5/BM25 (field weights 0,0,8,6,5,1,4),
NFKC+casefold query normalization with camelCase/snake_case/path splitting and 1–3-char n-grams
for non-ASCII (the canon's ASCII-only bug is genuinely fixed in code), hash-bound section
resolution (`span_hash` verified against live bytes; typed
`SourceChanged/Relocated/SourceMissing/Deny`), atomic per-generation registration
(`membrane ledger sync`), staleness re-hash at recall time. It also has the best evaluation asset
anywhere: `ledger-eval-v1` — 152 cases, 17 categories, 29 real documents, disjoint
train/dev/held-out with a validator — **which has never been run** (its own README says so;
`ledger-metrics.json` is `fixture-entrypoint-defined`).

**Gaps:** live retrieval is doubly off — `TRUSTED_LEDGER_FTS_RECEIPTS` is a hardcoded empty
allowlist (fail-closed until a real qualification receipt is minted) and the planner
doc-candidate provider is shadow-only behind default-off `MEMBRANE_DOC_PROVIDER_ENABLED` (cap 2);
no persisted link graph; relocation is a bare hit/miss with no move/rename history
(`prior_node_ids`/`relocation_reason` unimplemented); BM25 weights frozen without a documented
tuning run; the title-chain ablation is an unrun experiment; virtual session documents are
correctly non-recallable target-only.

**Fix:** run the corpus's dev split now (legacy_scan vs frozen ledger_fts), mint the first
qualification receipt, and activate; persist link/relocation history; tune and freeze BM25 weights
on the dev split with a published receipt.

---

## Did anything get lost from the old context system?

Checked line-by-line against the archived v1–v4 lineage. The v4 ADR's target — warm core, thin
doors, typed packets and receipts — is what Membrane *is*; the migration was faithful in shape.
Most hard-won mechanics survived verbatim, several relocated rather than deleted (scope-chain and
link-lane moved inside Cortex; transforms moved into Push; nine providers kept their lanes; skills
stayed a sealed provider).

### Genuinely lost or regressed

1. **Cross-machine replication v2** — immutable content-addressed events through git,
   deterministic LWW, permanent tombstones, per-machine re-embedding. Entirely absent. Membrane is
   single-machine today; the old system ran two. Deserves an explicit decision: revive the design
   as a Cortex pending item, or declare single-machine deliberately.
2. **The blind relevance spot-check pipeline** — the predecessor's calibrated 20-query judge with
   strict/useful rates and shadow-ranker comparison survives only as a code comment.
3. **The operational "is it earning" clock** — kill criteria reviewed on a schedule
   (`/context-metrics`), per-transform savings telemetry on the live path, and the
   availability×latency KPI over *all* prompts. The landed-capability invariant absorbed the
   *launch* half of the anti-Graphify contract; the *ongoing* half has no equivalent.
4. **Reserved admission lanes** — cross-provider starvation protection (memory 800 / skill 300,
   then global fill) has no analogue at the federation layer.
5. **Quarantine-before-destructive-prune** — partially violated: exact-duplicate pruning deletes
   directly.
6. **Blueprint's coverage denominator and six-state claim model** — the unified architecture's
   signature honesty guarantees exist only in narrower vocabularies.
7. **Frozen-experiment evidence** — the chunking tournament and the fused-RRF revert survive as
   decisions but not as carried evidence or stated qualification principles.
8. **File-level backup/restore** — the old snapshot discipline has no Cortex equivalent.

Nothing else material was found missing. Notably *not* lost, despite appearances: audit findings
and architect decisions as providers, skills-as-context, the scope-chain soft bonus, the link
graph, the effectiveness counters, and the SHA-aware veto.

---

## Priority queue

1. Fix the false-green CI lifecycle gate and the two stale lifecycle docs (canon drift is a
   defect — this one is CI-blessed).
2. Push: route production reduction through the real transform dispatch, with query metadata and
   telemetry.
3. Ledger: run the existing eval corpus → qualification receipt → activate FTS retrieval.
4. Pull: wire the sufficiency-contract caller so corrective retrieval runs live, then qualify
   fusion on a labeled corpus.
5. Cortex: relevance spot-check harness; backup/restore; duplicates through quarantine.
6. Adapt: real held-out corpus; feed the precision gate from real scorecards.
7. Blueprint: coverage disposition + doctor percentage; six-state claim verdicts; graph import.
8. Wire `install-release.ps1` into release CI (Appendix A coverage).
9. Decide the cross-machine story explicitly.

---

## Closure record (same day, 2026-08-29)

The priority queue was executed the same day across parallel agents plus companion sessions;
integration was verified by one serialized full-workspace run (`cargo test --workspace --locked`,
one borrow-check fix and one spotcheck-classifier fix applied during integration), `pnpm test`
(194 pass), the rewritten lifecycle gate, and the docs gate.

| # | Item | Outcome |
|---|---|---|
| 1 | CI lifecycle gate + stale docs | **Closed.** `check-lifecycle-conformance.mjs` now asserts Architecture B against code facts; `resident-lifecycle.md` and `hub-handoff.md` rewritten. |
| 2 | Push production wiring | **Closed.** `/federate` reduced_1 routes through the classify→transform dispatch with planner query metadata and unconditional telemetry (`push/selection.rs`, `push/prep.rs`). |
| 3 | Ledger qualification + activation | **Closed, promoted on evidence.** `ledger-eval-v1` run per hygiene rules (no dev tuning needed; held-out once): held-out MRR 0.60 vs 0.10, R@5 0.68 vs 0.14 for `ledger_fts` vs `legacy_scan`. Receipt minted and allowlisted; harness at `tests/ledger_eval_v1_harness.rs`; metrics file status `qualified`, decision `promote`. |
| 4 | Corrective retrieval caller | **Closed.** First-party sufficiency-contract path exercises the one-stage alternate-lane corrective action on the production federate path (`pull/federation.rs`, federation qualification tests updated). |
| 5 | Cortex quarantine + spot-check | **Closed / ported.** Duplicate consolidation routes through reversible quarantine (`dream.rs` + store); the predecessor's blind relevance spot-check is ported to Rust (`cortex_relevance_spotcheck.rs` + fixtures + tests), including the phase-3 prompt-hygiene classifier. |
| 6 | Adapt real held-out corpus | **Scaffolded.** `adapt/eval/build_real_heldout.py` + `n4_heldout/` + `tests/real_heldout_corpus.rs` landed by a companion session; corpus population from real sessions remains ongoing data work. |
| 7 | Blueprint honesty items | **Partially advanced** by a companion session (`blueprint.mjs`, `DOCUMENT-LIFECYCLE.md`, frontmatter lifecycle tests); coverage-disposition taxonomy and graph import remain open. |
| 8 | Installed qualification in CI | **Closed.** `release-candidate.yml` gained a gated `installed-qualification` job running `install-release.ps1`, skipping with a typed reason when prerequisites are absent. |
| 9 | Cross-machine story | **Open decision** — unchanged. |

*Companion capture: sPTC research note at
`docs/research/notes/sptc-speculative-programmatic-tool-calling.md`. Lineage baseline lives at
`D:\Claude\tools\.cache\memory\sync-repo\docs\archive\context-engineering-lineage\`.*
