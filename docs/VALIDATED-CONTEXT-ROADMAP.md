# Validated Context-Efficiency Roadmap

Validated 2026-07-26 against current Membrane source, installed Claude hook state,
fresh Blueprint graph, focused tests, Headroom, SuperCompress, LongCodeZip,
AgentDiet, LCM, OpenAI prompt caching, and Claude hook output replacement.

## Corrections to the proposed P0-P6 sequence

| Claim | Validated status | Decision |
|---|---|---|
| Blueprint `graph.db` contract is broken and degrades every prompt | Obsolete. `blueprint graph status` reports a fresh `blueprint-treesitter` generation; a live `graph candidates` call returned a fresh candidate set. Provider contract tests pass. | Remove as blocker. Keep freshness and provider-contract checks as regression gates. |
| `context-pulse` needs a repo-aware `blueprint_section` | Already shipped. `blueprint_section(repo)` reads the selected repository, and `--repo` passes that path. | Remove from roadmap. |
| `context-pulse cost` works fully offline | Partly false. Token counting falls back offline, but first-run cost is unavailable without the downloaded LiteLLM catalog. The workspace's intended bundled `tools/config/context-token-prices.json` is absent. | Keep as P0 repair: add a dated bundled price catalog, unknown-model handling, and source-age reporting. |
| Token Observatory needs to be built | Mostly obsolete. `context_session_inventory.py` already implements the census and has 15 passing focused tests. | Operationalize, schedule, and display the existing analyzer; do not create a second observatory. |
| PostToolUse PUSH still needs to be built | Partly obsolete. Hook and transform engine exist and are registered, but mode is unset/off, cohort receipt is absent, and installed `push_transform.py` differs from source. More importantly, the hook emits `additionalContext`, which appends compressed text beside the raw result instead of replacing it. | Treat as repair, hardening, and promotion. Use Claude's typed `updatedToolOutput` contract per tool shape. |

## Full ordered list

### P0 — Runtime truth and installation parity

1. Keep Blueprint graph health as a regression check, not a delivery blocker:
   `graph status`, live `graph candidates`, provider contract tests, and freshness
   receipt parity.
2. Reinstall hook sources atomically so installed `post_tool_push.py` and
   `push_transform.py` match workspace source.
3. Add a bundled, dated, offline price catalog used by both
   `context_session_inventory.py` and `context-pulse.py`.
4. Make pulse report price-source age, unknown models, analyzer report age, and
   collection coverage rather than silently implying complete cost attribution.

**Gate:** live graph candidate set is fresh; installed/source hook hashes match;
offline cost returns either a dated known price or an explicit unknown-model
status.

### P1 — Token Observatory operationalization

1. Extend the existing census only where fields are missing: input, output,
   cache read, cache creation/write, model, client, session, day, subagent share,
   largest tool results, and repeated file reads.
2. Emit daily machine JSON plus Markdown summary and expose the same artifact in
   `context-pulse`; do not duplicate aggregation logic.
3. Keep unavailable values unknown. Every ratio carries numerator, denominator,
   matched-session count, and coverage.
4. Add advisory session/model/day ceilings. No throttling or memory mutation.

**Gate:** deterministic fixture replay; fresh report produced without manual
invocation; cached and uncached economics remain separate.

### P2 — Trust, fidelity, and frozen eval gates

1. Move instruction, secret, origin, authority, and quarantine validation into
   the store layer so CLI, HTTP `/put`, and batch ingestion share one policy.
2. Add protected-span masks before ONNX selection for identifiers, errors,
   failing tests, paths, links, URLs, numeric facts, negations, quoted facts,
   code fences, and explicit user/system instructions.
3. Replace optional asset-skipping release tests with a fail-closed asset gate.
4. Build frozen fixtures for critical-span recall, syntax/structure validity,
   poisoning, cross-scope leakage, abstention, raw recovery, refetch regret, and
   OWASP ASI06.
5. Record real tokenizer counts, not whitespace or character estimates, for
   budget acceptance.

**Gate:** zero unauthorized influence; protected spans retained; structure
valid; task outcome non-inferior; required model assets present.

### P3 — Budget-driven transform APIs

1. Add `compress_to_budget(text, budget_tokens, options)` beside the rate API.
2. Keep the existing rate path byte-identical.
3. Add budgeted skeletonization with explicit degradation levels:
   current skeleton, signatures, public signatures, and path/symbol stub.
4. Return typed transform results containing requested budget, actual tokens,
   method, model/revision, latency, fallback, and invariant status.
5. Fail open to original content when output grows, structure fails, or protected
   spans cannot fit.

**Gate:** budget respected using the selected model tokenizer; output size is
monotone as budget shrinks; syntax and protected-span suites pass.

### P4 — Query-aware allocation and deduplication

1. Allocate budgets after all providers return. Providers supply candidates and
   scores; planner remains sole final-budget owner.
2. Normalize scores within compatible lane/provider classes before
   score-proportional allocation because raw provider scores are not comparable.
3. Give each admitted candidate an `allotted_tokens` field and a typed zero-score
   or below-floor drop reason.
4. Add canonical source hash plus normalized content hash dedup before
   admission. Preserve merged provenance and explicit winner/loser receipts.
5. Use Blueprint symbol, call, import, and test edges for code candidate
   dependency closure.
6. Defer LongCodeZip conditional-perplexity/AMI reranking until the deterministic
   AST/graph allocator loses held-out answers. Never adopt its Python/Torch
   package, regex splitter, or fine-grained surprisal pruner.

**Gate:** deterministic allocation; sum never exceeds lane/global budgets;
strict planner cap remains intact; code edit tasks can recover full source.

### P5 — Drop manifest and transform receipts

1. Emit counts and hashes for dropped identifiers, error lines, paths, links,
   numeric literals, and protected spans.
2. Record kept ratios, lossless/lossy class, route, before/after tokens, latency,
   fallback reason, source digest, compressor revision, and risk.
3. Mark risk high when any protected identifier, error, failing-test line,
   negation, or numeric fact is lost; high-risk output cannot enter the hot path.
4. Join planner selection receipts to client-finalized rendered and delivered
   counts.

**Gate:** golden manifests are deterministic, content-free where required, and
joinable from capture through model-visible delivery.

### P6 — Scope-bound reversible context store

1. Generalize the existing spill directory and `/anchor/retrieve` route instead
   of adding a parallel `/expand` subsystem.
2. Standardize `mr://anchor/<sha256>` handles with owner, scope, source identity,
   source hash, freshness, creation time, TTL, size, and transform receipt.
3. Add exact retrieval first; optional indexed/BM25 search only after exact
   retrieval is reliable.
4. Emit retrieval, miss, expiry, and expansion-regret telemetry.
5. Harden permissions, atomic writes, bounded reads, cleanup, and deterministic
   missing-anchor errors.

**Gate:** transformed output round-trips byte-identically; cross-scope retrieval
fails; expired or stale anchors return typed errors.

### P7 — Typed structural router

1. Route captured content into JSON/JSONL, logs/test output, search/grep,
   diff/patch, tables/CSV, config/YAML/TOML, HTML, code, prose, or unknown.
2. Use lossless transforms first: exact dedup, constant-field factoring,
   schema/header folding, repeated-row/log collapse, and superseded-result
   removal.
3. Preserve errors, anomalies, distribution boundaries, requested entities,
   dependency neighbors, recent edited code, and structural delimiters.
4. Apply extractive or learned prose compression only when deterministic
   reduction cannot meet budget.
5. Fail open to original content on parse failure, growth, or invariant failure.

**Gate:** per-type fixtures preserve structure and critical evidence while
reducing real model tokens.

### P8 — Cache-stable finalization

1. Put stable instructions, tool schemas, and unchanged history first; put
   mutable task/context packets last.
2. Freeze previously delivered prefix byte-for-byte and compress only new delta
   until context pressure requires a controlled rewrite.
3. Record prefix hash, cache key/breakpoint, break reason, cache write tokens,
   cached read tokens, uncached tokens, and cache-adjusted cost.
4. Queue low-value pruning when immediate rewriting would destroy a valuable
   provider cache prefix.

**Gate:** trace replay proves lower cache-adjusted cost and latency, not merely
fewer raw tokens.

### P9 — Real PostToolUse PUSH path

1. Update the existing hook to emit `updatedToolOutput`, matching each built-in
   tool's structured output schema; use `updatedMCPToolOutput` only where needed.
2. Preserve raw output in the scope-bound anchor store before replacement.
3. Apply exact-repeat replacement, typed routing, budget transform, manifest,
   anchor, and non-growth checks.
4. Use PostToolBatch or host finalization for batch-level supersession; deleting
   a spill file is not transcript error purging.
5. Start in shadow mode, fix source/install parity, issue a signed cohort
   contract, then promote a bounded candidate cohort.

**Gate:** model sees replacement instead of raw-plus-compressed duplication;
raw recovery succeeds; cache-adjusted input falls; task quality, tool calls,
wall time, and expansion regret remain within preregistered margins.

### P10 — Session compaction and continuity

1. Add an immutable raw transcript store plus recursive summary DAG/materialized
   views for provider-neutral continuity.
2. Use deterministic soft/hard thresholds, protected recent tail, asynchronous
   compaction, atomic swaps, bounded expansion, and deterministic fallback.
3. Reuse the existing episodic memory tier for session-close packets; do not
   promote summaries automatically into semantic/procedural memory.

**Gate:** held-out long session resumes with fewer rereads and non-inferior task
outcome; every summary has lineage to exact originals.

### P11 — Promotion and ongoing learning

1. Replay saved production traces across no compression, current transforms,
   typed structural routing, query-aware routing, cache-stable mode, and session
   compaction.
2. Require task-answer equivalence, critical-span recall, structure validity,
   anchor recovery, poisoning resistance, p50/p95 overhead, cache-adjusted cost,
   tool-call count, wall time, and refetch/expansion regret.
3. Learn from retrieval/expansion behavior only after delivered-to-used
   attribution is calibrated on human labels.
4. Keep policy-affecting promotion separate from read-only reporting and
   replication schedules.

**Gate:** pre-registered non-inferiority contract passes on a bounded cohort;
rollback restores raw delivery without data loss.

## External adoption decisions

| Source | Decision |
|---|---|
| Headroom | Adapt typed routing, deterministic structural reducers, fail-open/non-growth guards, cache-stable delta mode, and local reversible handles. Reject wholesale proxy/memory/telemetry stack. |
| SuperCompress | Adapt segment → score → dependency closure → verifier shape only. Reject hosted API/CCR and code copying while license and tenant-isolation issues remain. |
| LongCodeZip | Defer optional AMI reranking behind code-context evals. Reject runtime package, regex splitting, fine pruning, and hot-path GPU dependency. |
| AgentDiet | Adopt trajectory waste classes—useless, redundant, expired—as hot-path pruning taxonomy. |
| LCM/Volt | Adapt immutable originals, summary DAG, deterministic thresholds, atomic compaction, and bounded expansion after P9 is proven. |

## Verified checks

### Implementation progress (2026-07-26)

- **P0 repair:** bundled dated price catalog added; forced-offline
  `context-pulse cost claude-sonnet-4-5` reports bundled source and `$3/M`.
  Focused Python suite: 17 passed.
- **P3 budget APIs:** `compress --budget`, `skel --budget`, and `prep --budget`
  are implemented. Protected-span overflow returns source with `budget_met=false`
  rather than dropping critical data. MemRight library: 254 passed; CLI: 42 passed.
- **P5 receipts:** deterministic, content-free drop manifests now record dropped
  identifier/error/numeric counts, kept-identifier ratio, and risk in direct
  transform telemetry and prep manifests.
- **P4 allocation interface:** planner blocks and receipts now carry deterministic
  score-proportional `allottedTokens`, preserving each admitted source-kind
  lane total. MemRight core: 162 passed.
- **P6 anchor handle:** `runc` spills are now atomically published under a
  content SHA-256 filename and emit `mr://anchor/<sha256>` beside the raw path;
  `memright expand <anchor> --spill-dir <dir>` and authenticated `POST /expand`
  recover exact bytes.
- **P9 source hook:** cohort-approved PostToolUse now emits shape-preserving
  `updatedToolOutput`, never append-only `additionalContext`; prose engine calls
  use `compress --budget`.
- **P0/P9 deployment:** `setup-workspace.py --hooks-only` installed matching
  source hashes for both PUSH hook modules; registration remains cohort-gated.
- **P7 initial router:** PUSH preserves JSON and unified diffs verbatim rather
  than applying lossy text reduction.
- **P4 finalization:** planner `allottedTokens` now survives finalizer block and
  receipt reconciliation, ready for per-block budget transforms.
- **P8 telemetry:** finalizer records a cache-stable prefix SHA-256 excluding
  per-turn trace identity, enabling cache reuse/break analysis.
- **P6 lifecycle:** every runc anchor now has atomically published JSON metadata
  with schema version, handle, digest, creation time, and size.
- **P6 expiry:** `POST /expand` returns typed 410 for expired anchor metadata.
- **Regression gate:** MemRight library 256, core 162, and cross-component
  Python contract suites 26 tests pass.

- Blueprint graph: fresh `blueprint-treesitter` generation; live candidate set
  returned with `freshness.stale=false`.
- Blueprint provider tests: 6 passed.
- PostToolUse PUSH tests: 5 passed.
- Token census tests: 15 passed.
- `context-pulse --repo /Volumes/D/claude/membrane --json` reported the selected
  repository, proving repo-aware wiring.

### Critical Files for Implementation

- `engine/crates/memright/src/compress.rs`
- `engine/crates/memright-core/src/planner.rs`
- `engine/crates/memright/src/serve.rs`
- `/Volumes/D/claude/tools/hooks/post_tool_push.py`
- `/Volumes/D/claude/tools/pipelines/memory/context_session_inventory.py`

## 2026-07-27 — activation, burn attribution, and one dropped item

### Shipped

- **Token Observatory** (`tools/pipelines/memory/context_burn.py`, parent workspace, surfaced as
  `context-pulse burn`). Per-day/per-model input, output, cache-read and cache-write tokens,
  cache-hit ratio, calculated cost, top sessions. First live run: $2,064 over two days, 97% cache
  hit, top session $290. Now emitted daily by the existing read-only Observatory lane — no new
  scheduled agent was needed.
- **PUSH lane activated** behind the cohort gate, with session-stable 50/50 assignment.
- **ASI06 memory-poisoning smoke test** (`engine/crates/memright/tests/memory_provider.rs`).
- **Blueprint freshness** rebound to HEAD with a repo-revision rebuild trigger
  (`tools/blueprint-refresh.sh`, installed as post-commit/post-merge/post-checkout).

### Defects found by activating rather than reasoning

Each of these made a shipped feature a silent no-op, and none was visible from the code alone:

| Defect | Effect | Fix |
|---|---|---|
| Gate read `payload.cohort`, which no harness sets | PUSH lane unreachable in every session | `assign_cohort` derives a stable arm from the session id |
| `_result` did not read `stdout` | Bash — the largest sink — always `non_text_result` | `stdout` added to the key list; sibling keys preserved |
| Anchor named a bare filename | Elided output unrecoverable by the model reading it | Marker carries full path, sha256, elided size |
| Claude usage records repeat per request | 2.6x cache-read inflation ($9,921 vs $2,064) | Dedupe on `requestId` |
| Codex accounts cumulatively | One session summed to 23.18B input tokens | Take the final cumulative total, net out cached input |

### Contract amendment

The PUSH cohort gate moved from 40% reduction / 5pp quality margin to **>=20% reduction / <=1pp
non-inferiority**, amended before any data existed (`push.jsonl` was absent). A 5pp margin would let
a measurably degrading compressor ship; a 40% floor would reject a genuine 25% win at zero quality
cost. The live planner cohort preregistration is untouched.

### Dropped: the server-side compaction probe

Anthropic's server-side compaction is `compact_20260112` under beta header
`context-management-2025-06-27`, and it is **Messages API only** (verified against the context
editing docs). It is not applicable here for two independent reasons, so it is removed from the
plan rather than deferred:

1. **Membrane owns no Messages API call path.** It is a context layer for harnesses; Claude Code
   performs its own compaction. There is no request for the parameter to ride on.
2. **It cannot be tested on the free tier.** The ClaudeCodeX gateway routes to MiniMax and Alibaba
   endpoints, which expose Anthropic-*compatible* APIs but cannot implement Anthropic's own
   server-side context management. Running the probe there would prove nothing.

It becomes relevant only if the studio builds a direct Messages API agent, at which point it is
that project's decision, not this one's.
