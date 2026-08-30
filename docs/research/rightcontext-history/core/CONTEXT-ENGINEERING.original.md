# Context Engineering — Architecture & Setup

Single source of truth for the workspace **context economy**: what enters the model's working set,
what leaves it, what is recalled into it, and what persists between sessions. Renamed from
`COMPACTION.md` (2026-06-30) because compaction is only **one of three families** here — naming the
whole stack after one family hid the other two (retrieval and curation).

This doc is written to be self-contained: a human or an LLM with no prior context can read it top to
bottom, understand every surface, and stand the whole system up from scratch (§8). Skills point here
instead of re-explaining any of it inline.

---

## 0. TL;DR

- **Three families, one goal:** keep exactly the right tokens in attention. **Compaction** shrinks
  what's in transit, **Retrieval** pulls the right durable thing in, **Curation** keeps the durable
  store lean.
- **Durable store = one SQLite + vector engine, and the DB IS THE SOURCE OF TRUTH (flipped
  2026-07-02)** — canonical Rust source is `tools/memright/` in this workspace repo; CodeRight
  consumes exact git-revision pins of `memright` and `memright-core`. The verified release binary
  is `tools/bin/memright.exe`: scoped per project, embedded with **EmbeddingGemma-300M-Q4** (768-dim;
  BGE retired 2026-07-02), **scope-chain soft bonus (0.02 per rank, `533f836b`)** so stronger
  global/parent memories can beat a weak exact-scope, recall-savings + transform logs (§10).
  Markdown is an engine-generated EXPORT (audit + sync), not authority — see §4.1.
- **Write paths:** memories → `memright put` / `POST /put` (agents) or the dashboard at
  `http://127.0.0.1:47851` (humans; `MEMRIGHT_PORT` is the canonical override). Knowledge-doc bundles → `tools/lib/skill_emit.py` (`concepts` →
  OKF bundle → ingested). No skill invents its own markdown/JSON or bypasses the engine.
- **Three hooks automate it best-effort** (no migration lag when they run): a recall hook injects on
  every prompt, a write-through hook ingests every memory/OKF write, and the same write-through hook
  auto-routes known knowledge docs through `skill_emit`. **All three are fail-open** — when installed and
  running they auto-route; a hook that's absent or errors silently does nothing (enforcement is
  best-effort, not unbypassable — see §6).
- **Engine is Rust, unified (cutover 2026-06-30, extraction 2026-07-01).** The workspace runs the
  `memright` binary on `:47851`; the Python `mem.py` is retired. `memright` is ONE engine
  (tiers/retriever/routing/effectiveness/dream/quantized-persistence, depends only on `coderight-memory`
  primitives) **extracted into the workspace-owned standalone, publishable crate** — CodeRight is a
  git-pinned consumer of `memright`, not the source owner, and the workspace runs the
  exact same engine code over its own DB. This is not two implementations converging on a shared
  format; it is one codebase, two data stores (workspace `memright-engine.db`, CR `evolution.db`), no
  mixing. See `coderight/docs/plans/2026-06-30-memory-engine-unification.md`. **The deterministic
  transform layers (2/3/5) merged into the same crate 2026-07-02** — `skel`/`compress`/`prep`/`runc`
  verbs + `curate` (plan: `2026-07-01-context-engine-unification.md`); workspace call-sites flip in the
  pending Task-7 cutover, and per-layer measurement (`transform_log`) + the effectiveness loop (`/get`)
  are the queued Task 8.

---

## 0a. Status legend (read before trusting any claim below)

This doc braids the **conceptual architecture** with the **current implementation**. Every
implementation claim carries one of these tags; an untagged sentence is conceptual framing, not a
shipped-state assertion. Verified against the Rust `memright` code 2026-07-01 after a Codex review
caught the doc describing the retired Python `mem.py` schema as if current.

- **`[Live]`** — wired and exercised in the running `memright` engine today.
- **`[Partial]`** — partly wired; specific gap named inline.
- **`[Engine-not-wired]`** — the capability exists in the Rust code but no CLI verb / serve route / call
  path reaches it from a real trigger.
- **`[Target]`** — desired convergence state; **not built**. Do not implement against it as if it exists.
- **`[Deprecated]`** — described a prior implementation (usually Python `mem.py`); retained only as history.

---

## 1. The three families

One goal: the right tokens in attention, nothing more.

- **Compaction (PUSH — shrink what's in transit).** Layers 1–6. Squeeze each thing that flows.
- **Retrieval (PULL — bring the right thing in).** Layer 7. Rank + inject from the durable store.
- **Curation (PERSIST — keep the store lean + useful).** Layer 8. Lifecycle of the durable store.

The families share **three primitives** — that's what makes this one stack, not eight tools:
`skel` (code AST), `compress`/`okf` (prose + structure-safe link graph), `embed`/vector (recall).

> **`[Target]`** A durable entry is *intended* to store `{ full, skel, embedding, okf-links }` so the
> same representations serve all three families. **Today the Rust `memright` entry stores `{ content,
> keywords, embedding, embedding_q }` only** — no `skel`, no `okf-links` column (see §4.2). That richer
> model is the convergence target, not the current schema.

---

## 2. The eight layers

| # | Family | Layer — what flows | Tool | What it does | Trigger |
|--:|---|---|---|---|---|
| 1 | compaction | **my reply → Adrian** | `brief` (skill) | ruthless prose editing, no model | always-on policy |
| 2 | compaction | **command output → my context** | `memright runc` (shim: `runc`) | caps noisy stdout to head+tail, caches full to `.cache/runc/`, returns a pointer | noisy cmds (tests/builds/logs) — **[Live] unified 2026-07-02** — shims keep the old command names; Python/Node copies retained one release as rollback |
| 3 | compaction | **file → agent context (INPUT)** | `memright prep` + `memright skel` (shims: `prep-context`, `skel`) | routes each input: code→`skel`, prose→`compress`, tiny→copy | fan-out SURVEY/SYNTHESIS reads — **[Live] unified 2026-07-02** — shims keep the old command names; Python/Node copies retained one release as rollback |
| 4 | compaction | **orchestrator → agent (A2A)** | machine-minimal directive (hook) + structured body | terse spawn prompts; hook auto-prepends to every Agent spawn | every spawn (automatic) |
| 5 | compaction | **agent doc output → future agent input (OUTPUT)** | `memright compress` + `okf.py emit` (library) | OKF bundle (concept-files + `type` frontmatter + link graph), structure-safe prose compression | doc-heavy skill artifacts — **[Live] unified 2026-07-02** — shims keep the old command names; Python/Node copies retained one release as rollback |
| 6 | compaction | **my running context** | harness `/compact` + `context-budget` planner | provider-owned summary/offload plus content-free PreCompact/PostCompact evidence | harness threshold; planner observes pressure but does not silently promote a new threshold |
| 7 | **retrieval** | **durable store → context (RECALL)** | `memright recall` (EmbeddingGemma via fastembed/onnxruntime, §4.5) | **`[Live]`** rank by cosine over a scope-chain-filtered hybrid candidate set; logs every recall (savings, §10). **`[Target]`** the okf-link/decay/effectiveness terms + skel-by-default (§4.6) | session start (hook) + on-demand |
| 8 | **curation** | **durable store lifecycle (PERSIST)** | `memright curate` + `mem-recency` (skill) | **`[Live]`** `memright curate` CLI verb runs `MemoryStore::dream_now` — dedupe + relative-date normalization + low-value prune, all **in-place under the primary's own id/scope/counters** (no generated `dream-` ids; reshaped 2026-07-04 after the first scheduled run minted 190 opaque colliding ids); **[Live 2026-07-04] scheduled** in `daily-sync.sh`; **`[Target]`** decay, contradiction-merge, digest, serve route | scheduled + on-write |

Underlying primitives: `skel.py`/`skel.mjs` (code AST, reversible, caches original), `compress.py`
(LLMLingua-2 prose, CPU, local), `okf.py` (structure-safe bundle + link graph). `runc`/`skel`/
`compress` are the **safe local subset of Headroom** — no network, no traffic interception.
Layers 7–8 use a real **embedder** — EmbeddingGemma-300M-Q4 via fastembed on the installed
onnxruntime, offline after the first model pull.

### 2.1 One global adjunct-context planner **`[Live candidate 2026-07-11]`**

`tools/lib/context_budget.py` is the single deterministic allocator for context MemRight and
Blueprint can actually control. It measures transcript-file pressure and allocates one bounded
adjunct budget across memory previews, Blueprint survey material, and command/tool output. The
same decision vocabulary is used by Claude, ClaudeMM, Codex, and Blueprint:
`pressure_band`, `mode`, `memory_chars`, `blueprint_chars`, and `tool_output_chars`.

- Claude's `recall_planner.py` and Codex's `tools/codex-brief-plugin/recall_planner.js` are the live
  prompt adapters. They request the bounded federation packet in on mode and fall back to the
  legacy semantic-recall implementation when the graph is dirty, the service is unavailable, or
  the packet is degraded. The Rust routes validate and enforce all recall bounds.
- Blueprint task briefs invoke the same planner and cap Read-First file count from the allocated
  Blueprint characters. Phase 2a VERIFY and every edit-intent read still use full source.
- Claude and Codex register `PreCompact` and `PostCompact` hooks. They log only surface, stable
  session hash, trigger, transcript bytes, summary characters, and summary SHA-256. They never log
  transcript or compact-summary content and never re-inject the summary a second time.
- `daily-analysis.py` joins planner pressure and paired compaction counts to provider-token data.

This does **not** pretend to own provider conversation history. Codex and Claude still perform the
actual Layer-6 compaction. Earlier-compaction thresholds remain a measured candidate to promote only
after the new evidence stream can show token improvement without completion/error regression.

**Windows candidate activated 2026-07-11 (reversible):** Codex reported a 353,400-token model
window, so `~/.codex/config.toml` sets `model_auto_compact_token_limit = 212000` (60%). Claude and
ClaudeMM use `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=60` in `~/.claude/settings.json`. Both settings load on
a new task/session. Remove those two overrides to return to provider defaults. This is an active
candidate, not a savings claim: provider-observed noncached input and completion/error guardrails
must show the requested ≥40% reduction before the policy is called successful.

---

## 3. The routing rule (compaction family)

> **code → `skel` · prose → `compress` · markdown-with-refs → `okf-compress` (structure-safe) · command output → `runc`.**

**Naming disambiguation (Codex review):** "`okf`" means two different things — keep them distinct:
- **`okf-compress`** = structure-safe prose *compression* (protects `path:line`/code/`[[link]]` spans
  while token-dropping). This is what the routing rule above means, and what `prep-context.py` actually
  calls for non-code text. **`[Live]`**
- **`okf-bundle`** = emitting a durable OKF *bundle* (concept files + frontmatter + link graph) — the
  layer-5 OUTPUT path via `skill_emit`. Different operation. **`[Live]` for emit; the link *graph* is
  not stored in the engine (§4.2 `[Target]`).**

Note: `prep-context.py` routes non-code, non-tiny text through `okf-compress`, **not** `okf-bundle` —
the input router compresses, it does not emit bundles.

`skel` (AST) and `compress` (token-drop) never overlap — code vs prose. `okf-compress` is `compress`
wrapped to protect `path:line`/code/links (use it for any markdown that carries refs). Never run
raw `compress` on code (it breaks syntax) or on ref-bearing markdown (it breaks the refs).

### 3.1 Rust-vs-Python parity evidence (call-site flip, 2026-07-02)

Recorded before flipping workspace call-sites off the Python/Node originals, so the rollback has a
concrete basis rather than a vibe:

- **`skel`** — semantic parity: 9/9 defs captured on the comparison file. The Rust output is leaner and
  is the canonical form going forward — it drops `skel.py`'s header + docstrings that the Python version
  kept. One asymmetry favors Rust outright: `skel.py` **failed silently** on `skill_emit.py`, where the
  Rust skeletonizer succeeded.
- **`prep`** — contract parity: kinds vocabulary, per-kind key sets, output filenames, and the branch
  order (missing → copy → skel → compress) all match `prep-context.py`. The fallback branch differs only
  where the underlying skeletonizer itself succeeds vs fails (inherits the `skel` asymmetry above).
- **`runc`** — behavior parity: head/tail capping + elision marker, spill-to-file, and child exit-code
  preservation all match `runc.mjs`.
- **`compress`** — keep-rate band: Rust heuristic ~62% vs Python LLMLingua ~65%. Token counts aren't
  directly comparable (different tokenizers). The LLMLingua-ONNX path stays deferred pending
  `transform_log` volume/savings data (§10) before it's worth the 709 MB deploy.

---

## 4. The durable memory engine (layers 7 + 8)

The center of gravity. Everything durable — memories, blueprint OKF, (soon) transcripts — lives in
ONE engine so semantic recall spans the whole corpus instead of grepping siloed markdown.

### 4.1 Where it lives

| Thing | Path |
|---|---|
| Engine | Canonical Rust source `tools/memright/`; verified release at `tools/bin/memright.exe`; CodeRight consumes exact git pins |
| SQLite DB | `tools/.cache/memory/memright-engine.db` (gitignored; path is `MEMRIGHT_DB`-overridable) |
| Memory pointer (per project) | `~/.claude/projects/<slug>/memory/MEMORY.md` — the ONE md the system authors: the harness auto-loads it, and it redirects every agent to the engine (`put`/`get`/dashboard). Never ingested. |
| Markdown export (audit + sync) | `<WORKSPACE_ROOT>/memory-mirror/` — generated FROM the DB by `memright export-md` (§13a); git-tracked, human-diffable, the restore source |
| Blueprint OKF (per repo) | `<repo>/.agent/okf/**/*.md` + `<repo>/.agent/START-HERE.md` (gitignored) |
| Embedder model cache | scheduler-owned `tools/.cache/fastembed/` (`HF_HOME`), pulled once; cwd `.fastembed_cache/` is legacy/manual |
| Human UI | `http://127.0.0.1:47851` — embedded dashboard (search/browse/edit/delete/metrics) served by `memright serve` |

**Corpus size (dated — will drift):** 999 entries as of **2026-07-10 01:32Z** (all
`embeddinggemma-300m-q4`, zero hash-space rows, zero `dream-` ids; one newer remote event was applied
during the replication-v2 cutover). Re-check
with `memright list | wc -l`; do not treat this number as current.

Canonical overrides: `MEMRIGHT_DB`, `MEMRIGHT_PORT`, and `MEMRIGHT_API_TOKEN_FILE`. The tracked
`tools/lib/memory/runtime.json` pins service identity/host/port; legacy `WORKSPACE_MEMORY_PORT`
is compatibility-only,
`WORKSPACE_EMBED_MODEL` (default EmbeddingGemma-300M-Q4; `bge-small-en-v1.5` selectable),
`WORKSPACE_ROOT` (default `D:/Claude`).

**THE DB IS THE SOURCE OF TRUTH (flipped 2026-07-02, Adrian's directive — plan
`coderight/docs/plans/2026-07-02-db-first-memory.md`).** Agents write THROUGH the engine
(`memright put` / `POST /put`); humans use the dashboard; markdown exists only as the engine-generated
`export-md` tree (audit trail + cross-machine sync medium + disaster restore). The per-project
`MEMORY.md` pointer is the single remaining hand-shaped md — it exists because the Claude harness
auto-loads it, and its content is the instruction to use the engine. The ingest hook remains as a
safety net (an agent that writes a legacy md file still gets captured into the DB), but on conflict
the DB row wins.

### 4.2 Data model

**`[Live]` — current Rust `memright` schema v10** (`tools/memright/crates/memright/src/memdb.rs`,
reviewed 2026-07-16). Migrations are versioned and transactional. Opening the DB either reaches the
complete v10 schema or leaves the previous version intact; a newer unknown schema version fails closed:

```
memories(
  id           PRIMARY KEY,     -- scope-qualified: "<scope_id>/<name>"
  tier         TEXT,            -- Working | Episodic | Semantic (cognitive tier, NOT "type")
  content      TEXT,
  keywords     TEXT,            -- space-joined lexical keys
  score        REAL,
  created_at   TEXT,
  updated_at   TEXT,            -- mutation clock; v5 backfills legacy rows from created_at
  access_count INTEGER,         -- [Live] persisted via /use and /get -> record_use (survives reopen)
  inject_count INTEGER,         -- [Live 2026-07-02] +1 per /recall hit returned (Task 8b; migration adds it)
  embedding    BLOB,            -- f32 vector; dimension is model-defined (768 for the live default)
  embedding_q  BLOB,            -- TurboQuant i8 (the low-RAM path)
  scope_id     TEXT DEFAULT 'global',  -- project slug; see §4.4
  content_hash TEXT,
  embed_model  TEXT,
  source_ids   TEXT DEFAULT '[]'       -- durable consolidation/replication provenance
)
recall_log(id, ts, scope, query_chars, hit_count, full_chars, injected_chars,
           source, query_preview, client, session_id, cwd_scope, hook_event,
           trace_id, client_visibility, traffic_class,
           candidate_hits, admitted_hits)
                                  -- [Live] savings + attribution + exact ranking/admission replay.
                                  -- `observe=false` evaluation requests write no row and increment
                                  -- no injection counters; traffic_class separates production/smoke/eval.
transform_log(id, ts, verb, scope, before_chars, after_chars, meta)            -- [Live 2026-07-02] L2/L3/L5+curate (Task 8a)
deletions(id, deleted_at)                                                      -- permanent replication tombstones; a newer explicit put clears the local ledger row
memory_event_log(event_id, ts, event_kind, memory_id, surface, session_id,
                 trace_id, scope_id, quantity, meta)
                                  -- [Introduced v6; retained in live v10] content-free put/update/inject/get/delete/
                                  -- curate/outcome lifecycle events, committed atomically.
context_policy_assignment(session_id, surface, policy_version, cohort,
                           assigned_at, task_class)
                                   -- [Introduced v6; retained in live v10] deterministic session holdout assignment;
                                   -- no prompt or memory content.
context_feedback(trace_id, candidate_id, content_sha256, outcome, verified,
                 verdict_ref, scope_id, ts) -- [Live schema v7] SHA-aware recall veto
links(src_id, dst_slug)                     -- [Live schema v8] parsed [[wikilinks]]
skills(name, description, body, body_sha256, resources, updated_at) -- [Live schema v9]
memory_quarantine(id, tier, content, keywords, score, created_at, updated_at,
                  access_count, embedding, embedding_q, scope_id, inject_count,
                  content_hash, embed_model, source_ids, quarantined_at, reason)
                                   -- [Live schema v10] complete-row reversible quarantine
```

**`[Target]` — the richer model this is converging toward** (and what the retired Python `mem.py`
had). **None of these are in the Rust schema yet** — do not query them:

```
+ name, description, skel, type, domain, source_path     -- richer provenance + the skel for PUSH
+ success_count                                          -- remaining quality input; lifecycle access timestamps are [Live v6]
+ confidence, provenance, version, superseded_by         -- supersede-on-contradiction (curation)
+ pinned                                                  -- score bump
+ kind  (memory|blueprint|transcript)                    -- see §4.3 — NOT a column today
```

> The Rust engine remains deliberately lean. Durable mutation time, lifecycle attribution,
> deterministic control/candidate assignment, recall replay telemetry, per-memory access time,
> CodeRight `record_run` success/failure outcomes, and the per-candidate feedback rail
> (`context_feedback` + SHA-aware veto in shared `recall_scored`), the persisted link graph and
> bounded depth-1 neighbour lane, engine-served skills, and reversible quarantine are live.
> Kind-bucketing and supersession remain `[Target]`.

### 4.3 `kind` — the unified corpus  **`[Target]`**

The intent: one `kind` column buckets `memory` / `blueprint` / `transcript` so recall spans the whole
corpus from one table. **Today there is no `kind` column.** Blueprint OKF and memories are
distinguished only by their scope-qualified `id` prefix (`<scope>/bp/...` vs `<scope>/<name>`), and
`migrate` vs `migrate-blueprint` ingest them separately into the same flat table. The shared `links`
graph exists, but it relates rows without adding a `kind` discriminator.

### 4.4 Scope model

**Slug.** A project's `scope_id` is Claude's own project-slug: each `:` `\` `/` in the cwd becomes
`-`, with **no collapsing**, and the leading Windows **drive letter is normalized to uppercase** so a
lowercase-drive cwd can't fork a project's memories into a second scope:

```
D:\Claude\coderight   -> D--Claude-coderight
d:\Claude\coderight   -> D--Claude-coderight   (normalized, same scope)
```

`normalize_scope()` applies this in `path_to_scope`, `_infer_scope`, `migrate_all`, the recall
hook, and (since 2026-07-05) the CLI `recall --scope` arg — the five places a slug is produced
(the CLI was the unnormalized fifth site: a lowercase-drive `--scope` silently produced an empty
chain). (A pre-fix fork, `d--Claude-mailright`, was consolidated
2026-06-30.)

**Chain recall.** A scope sees **itself → its ancestors (path-slug prefixes that actually hold rows)
→ `global`**, never its siblings:

```
D--Claude-coderight  ->  [D--Claude-coderight, D--Claude, global]
```

So coderight passively recalls its own + workspace-wide + global memories, but **not** heardright's
(sibling isolation). **`[Live]`** the scope *filter* (chain membership) — verified. **`[Target]`** the
"closer scopes get a `0.05 * depth_rank` boost" — `recall_scored` filters by scope then sorts by pure
cosine; it applies **no** depth boost today.

**Cross-project. `[Live]`** For an explicit cross-project question, pass `cross=["D--Claude-heardright"]`
to add specific sibling scopes on demand — opt-in, never passive (serve.rs merges them into the chain).

### 4.5 Embedder

**`[Live]` (upgraded 2026-07-02)** EmbeddingGemma-300M-Q4 (768-dim, 2K context, multilingual, <200MB
resident) via **fastembed 5 on the installed onnxruntime** — replaced BGE-small-en-v1.5 (33M/384-dim/
512-ctx, 2023-era). Prompt-trained: documents embed as `title: none | text: …`, queries as
`task: search result | query: …` (encoded in `FastEmbedder`, which also closes the asymmetric-query
gap for good). Qwen3-Embedding-0.6B (1024-dim, 32K ctx, higher ceiling, 2× RAM) selectable via
`WORKSPACE_EMBED_MODEL`. **Embedder swaps re-embed from DB content alone — `memright reindex`, no
files needed.** `default_embedder` is now FAIL-LOUD: a failed ONNX init aborts instead of silently
degrading to the hash embedder (which had quietly filled 467/512 rows with wrong-space 256-dim
vectors — discovered and repaired by reindex 2026-07-02; `MEMRIGHT_ALLOW_HASH=1` is the explicit
opt-in for degraded mode). Old text (BGE) below for history:

**`[Deprecated]`** BGE-small-en-v1.5 (384-dim) via **fastembed on the installed onnxruntime** — real semantic
vectors, offline after the first model pull. A stronger model swaps in behind the `Embedder` trait with
no schema change.

**`[Live]` the asymmetric query path (fixed 2026-07-02, Task 8c; upgraded with EmbeddingGemma).**
`FastEmbedder` overrides `embed_query` with the selected model's query prefix and `recall_scored`
calls it; documents use the matching document prefix. If recall quality shifts after a model change,
recalibrate the hook's `MIN_COS 0.40` against fresh data rather than assuming the old threshold.

### 4.6 Scoring

**`[Live]` today:** `recall_scored` delegates to `recall_scored_detailed`, filters to the explicit
scope chain before candidate generation, then does hybrid lexical + semantic candidate generation via
`MemoryRetriever::retrieve_hybrid` → **sort by `cos + SCOPE_CHAIN_SOFT_BONUS`** → reserve a bounded
depth-1 link lane → apply the shared SHA-aware effectiveness/contradiction veto → truncate. Filtering
first prevents a sibling-heavy corpus from consuming the candidate budget. The returned score is the
cosine for semantic hits; the scope bonus is internal to sorting and not surfaced. Detailed callers
also receive content-free `origin=semantic|link` attribution.
The bonus is 0.02 per scope-rank closer to the requester (self=0.04 → parent=0.02 → global=0.0 in a
3-link chain), so a clearly stronger global memory (e.g. 0.62 cos) can beat a weak exact-scope memory
(0.41 cos) — but a near-tie exact-scope (0.43 cos) still wins over a global (0.42 cos). Test guard
`recall_scored_uses_scope_as_soft_bonus_not_hard_sort` pins both directions (tie → exact wins;
clearly-better global → global wins). Replaces the 2026-07-05 hard scope sort that was too blunt
for global design/audit memories (`533f836b`).

Pre-2026-07-09 behavior, retained for context: a hard scope-rank sort was attempted and reverted
within 24h (it was over-broad — the soft bonus is the measured right architecture for this corpus).
The previous "sort by pure cosine" claim in this section is now stale; this paragraph is the new truth.

**TESTED AND KEPT (2026-07-05 — the reranker-gate data §10 asked for):** shipping the fused RRF
order end-to-end was tried and REVERTED same day. The pre-flip gate (30 known-useful targets vs a
frozen DB snapshot, old vs new binary) showed fused ranking degrading mean rank **2.37 → 6.07**
(+3.70; tolerance +1.0; 21/30 targets worse): the lexical RRF channel floods rank positions with
keyword-popular entries, displacing exact semantic matches. Fused-for-candidates +
cosine-for-order is the measured right architecture for this corpus (pinned by the
`recall_scored_sorts_by_cosine_over_fused_candidates` test). Caveat: the eval queries were
name-shaped (semantically biased); `recall_log.query_preview` now records real traffic, so any
retry must WIN on a real-query replay first. Full verdict:
`docs/plans/2026-07-05-memright-context-engineering-next.md`.

The link lane reserves at most `floor(limit/5)` slots (minimum one when enabled, maximum eight), so
direct semantic ranking always keeps at least 80% of the caller budget. Links never expand recursively.

**`[Target]` — the richer blend below is NOT wired** (needs the remaining `[Target]` columns from
§4.2: `last_used`, durable effectiveness inputs, and `pinned`):

```
score = cos
      + 0.06 * decay            # [Target] exp(-age/30d), needs last_used
      + 0.06 * effectiveness    # [Target] (use_count+1)/(inject_count+2), needs those columns
      + 0.04 * pinned           # [Target] needs pinned column
      + scope_depth_boost       # [Target] <= 0.05 closer-scope
      + graph_neighbor lane     # [Live separately] bounded one-hop candidates, not this additive score
```

`effectiveness` is the intended Graphify guard (an injected-but-never-used memory decays out). **Partially
wired** — an in-session `EffectivenessGate::should_inject` already filters injection on in-RAM usage
history, and `access_count` is now persisted (`POST /use` → `record_use`, survives reopen). **Still
`[Target]`:** the durable effectiveness inputs (`use_count`/`inject_count`/`success_count`) and reading
effectiveness into the recall *sort* — the sort is cosine-only today.

**Closure LIVE (shipped 2026-07-02 — unification-plan Task 8b).** Mechanical, no LLM judgment:
`inject_count` increments per hit RETURNED by `/recall` (persisted; an open-time migration adds the
column to existing DBs), and `POST /get {id}` / `memright get <id>` returns the FULL content and calls
`record_use` — an agent fetching the full text after seeing the ~200-char injected preview IS the
"this memory was useful" event. The recall hook injects the exact get command with every block, so the
fetch loop exists. Effectiveness = `(access_count+1)/(inject_count+2)` — computable now from persisted
columns; wiring it into the recall *sort* waits for accumulated data (data first, scoring second).

### 4.7 CLI surface

Post-extraction (2026-07-01) CLI is intentionally small — the engine's real surface area is the
`MemoryStore` Rust API, which the serve contract below exposes over HTTP:

```
memright <cmd>            # DB arg is REQUIRED: MEMRIGHT_DB env or --db. The implicit
                          # ~/.memright/memory.db fallback was REMOVED 2026-07-05 — it silently
                          # forked the corpus (fail-loud, embedder precedent).

serve            [--port 47851]             # resident recall/ingest service; runtime.json/MEMRIGHT_PORT are canonical
migrate                                      # import every ~/.claude/projects/*/memory dir (+ global)
migrate-blueprint                            # import every <WORKSPACE_ROOT>/*/.agent/okf bundle
recall <query>   [-k 6] [--scope <s>]        # one-shot recall; normalizes --scope, logs to recall_log
                                             #   with source='cli' (never bumps inject_count) [2026-07-05]
metrics                                      # token-savings report (see §10) — full vs injected chars
skel <file>                                  # L3 code skeleton (tree-sitter)        [merged 2026-07-02]
compress [--rate 0.5] [--no-onnx] [file]     # L3/L5 prose compression (heuristic default) [merged 2026-07-02]
prep <out_dir> <file...> [--rate --min-bytes]# L3 router, prep-context manifest parity [merged 2026-07-02]
runc [--head N --tail M] -- <cmd...>         # L2 exec + head/tail cap + spill        [merged 2026-07-02]
curate [--today YYYY-MM-DD]                  # L8 dream_now (dedupe + prune)          [merged 2026-07-02]
get <id>                                     # FULL content + records use (effectiveness) [shipped 2026-07-02]
put <name> [--scope S] [--tier T] [--file F] # DB-first WRITE path (stdin default) — embeds + persists [DB-first 2026-07-02]
delete <id>                                  # remove row + in-RAM entry                [DB-first 2026-07-02]
list [--scope S]                             # browse: access/inject/chars/tier/id      [DB-first 2026-07-02]
reindex                                      # re-embed EVERY row from DB content (embedder swap / repair) [DB-first 2026-07-02]
export-md <dir>                              # generate the canonical-scoped md tree FROM the DB (audit+sync) [DB-first 2026-07-02]
```

Transform verbs + `curate` log to `transform_log`; `metrics` now emits a `transforms` block and a
`curate` block alongside the flat recall keys (Task 8a). CLI transform logging is fail-open: an
unreachable DB skips the row, never the transform.

The L2/L3/L5 transform verbs merged with the context-engine unification (CodeRight `main` @ `c96b387e`);
Task 8 (measurement + effectiveness + `get`) shipped 2026-07-02 and **the deployed
`tools/bin/memright.exe` is the post-Task-8 build (11 verbs, including `get`)** — plan:
`coderight/docs/plans/2026-07-01-context-engine-unification.md`. **Workspace caveat (redeployed
2026-07-02):** the binary was redeployed with all 11 verbs live, and skill call-sites (blueprint, seo,
audit, brief) were flipped from the Python/Node copies (`prep-context.py`, `runc.mjs`, `skel.py`,
`compress.py`) to the `memright` shims the same day — those old copies are retained one release as
rollback, not the live path.

**Known gap (post-DB-first, 2026-07-02):** `add` (single-file CLI verb) / `selftest` / `benefit`
do not exist on this surface; the old serve `/add` route was **disabled** in commit `c4fb78b1` — it
now returns `410 Gone` with `{"error":"/add disabled; use the memright CLI for file ingestion"}`
because accepting arbitrary local file paths over HTTP was the wrong trust boundary (any loopback
process could request `{"path":"/etc/passwd"}` and have the contents read into the store; defense-in-
depth requires the route to refuse, not the producer to be polite). Local producers use
`memright put --file` instead — `ingest_memory.py` was rewired in commit `692c9b9` to call the CLI
rather than the HTTP endpoint, with `_memory_name` (sanitize) and `_scope_for_path` (project-slug
from path) helpers that preserve scope routing for `/memory/` and `/.agent/okf/` paths. `reindex`
and `export-md` SHIPPED with the DB-first pivot. The effectiveness loop is CLOSED mechanically
(§4.6): `/recall` persists `inject_count`, `get` records use, the recall hook injects the get
command — what remains open is DATA (let it accumulate, review via `/context-metrics`).

### 4.8 The resident service

`serve` keeps the engine resident so it's the *actual* injection source instead of a silo: it loads
the embedder (EmbeddingGemma, §4.5) **once at boot** and answers over localhost, so the per-prompt recall hook **avoids the repeated
cold model load** (the model init is the dominant cost; amortizing it across requests is the point).
**[Live 2026-07-10] bounded Axum service:** Tokio/Axum 0.8 replaces `tiny_http`. The server binds
IPv4 loopback only, admits at most 32 concurrent requests, includes queue time in a 30-second request
deadline, and hard-rejects bodies above 1 MiB with 413 before parsing. Blocking store work runs on
Tokio's blocking pool. The recall hook's watchdog remains a last-resort recovery path.
*(No latency bound is asserted here — "sub-100 ms" was unmeasured; if/when measured, cite it.)*

**Loopback is not an authorization boundary.** All non-health routes require a bearer token from
`MEMRIGHT_API_TOKEN` or `MEMRIGHT_API_TOKEN_FILE`; the scheduler wrapper creates the token with an OS
CSPRNG, writes it atomically, and ACLs it to the current user. Browser requests additionally require
an exact same-origin `Origin` (`127.0.0.1` or `localhost`, current port); native clients may omit
`Origin`. POSTs require `application/json`. Dashboard responses carry no-store, no-referrer,
nosniff, frame-deny, and restrictive CSP headers, and the token is never logged.

**`[Live]` 2026-07-09..10 hardening:** the route table is exact-match
on path (not `starts_with` — `/put` no longer matches `/puts`), the body parser distinguishes empty
(400 `empty json body`) from malformed JSON (400 `malformed json body`), and the CLI's
`try_service_post` returns `Result<Option<String>, String>` — only connection-refused permits a
direct-DB fallback. A timeout, post-connect transport error, or 4xx/5xx response **refuses** the
fallback and propagates the error. This closes the F-17 double-write/split-brain mode where a timed-
out request could still finish in the resident process while the CLI wrote the same mutation again.
It also closes the earlier mode where a service-rejected `put` would land in the DB without ever
reaching the in-RAM registry of the live serve process. The `/add` route
was disabled in `c4fb78b1` (410 Gone) for the path-traversal reason in §4.7.

**`[Live]` — the actual service surface** (`serve.rs`, verified 2026-07-10; all routes except
`/health` require bearer authentication):

```
GET  /health                  -> {"ok": true}
POST /recall  {query,k,scope,observe?,traffic_class?} -> [ {id, skel, type, scope, kind, score, cos} ]
POST /memory-candidates {...} -> warm in-process federation memory candidates
POST /verify-memory {...}     -> DB-provenance/content-hash delivery seal
POST /feedback {...}          -> persisted used|ignored|contradicted candidate outcome
POST /use     {id}            -> {"ok": true, "access_count": n}  # persists access_count
POST /get     {id}            -> {"id", "content", "access_count"} # FULL content + records use [Live 2026-07-02]
POST /compress{text,rate,no_onnx,scope?} -> {"out": "<compressed>"}# logs to transform_log
# /skel and /prep routes REMOVED 2026-07-05 (consumer-less post-cutover — §10.2's own rule fired;
# the CLI verbs remain). memright's dead compact.rs module was deleted the same day (zero callers).
GET  /                        -> the embedded DASHBOARD (html)      # search/browse/edit/delete/metrics  [DB-first 2026-07-02]
GET  /metrics                 -> the metrics JSON (same as CLI)     # [DB-first 2026-07-02]
POST /put     {name,content,scope?,tier?} -> {"put":"<id>"}         # DB-first write path  [2026-07-02]
POST /get     {id}            -> {id, content, access_count}        # full content + records use  [2026-07-02]
POST /delete  {id}            -> {"deleted": bool}                  # [2026-07-02]
POST /list    {scope?}        -> [{id,tier,chars,access,inject}]    # [2026-07-02]
POST /curate  {today?}        -> {consolidated_count,quarantined_count,pruned_count} # reversible low-score quarantine; duplicate prune remains permanent
POST /quarantine/list {}      -> {"ids":["scope/id",...]}
POST /quarantine/restore {id} -> {"restored": bool}               # full-row transactional restore
POST /policy/assign {session,client,policy_version,control_pct?}
                              -> {cohort,policy_version,control_pct} # persisted deterministic holdout
```

Observed `/recall` persists exact candidate/admitted replay data and increments `inject_count` for
every admitted hit. `observe=false` does neither. The dashboard at
`http://127.0.0.1:47851` is the human interface — search (semantic), browse, read, edit (re-embeds on
save), delete, savings chips. Localhost-only. (`/skel` and `/prep` were removed 2026-07-05 per that rule; the dashboard made `/list`/`/put`/`/get`/`/delete`/`/metrics`
consumed from day one.)

Caveats on the `/recall` fields (don't read more into them than exists):
- **`skel` is a real prose PREVIEW since 2026-07-05** (still not a code skeleton — that stays
  `[Target]`): `preview()` skips YAML/TOML frontmatter, HTML comments, code-fence markers, and
  short title-headings, then takes the first ~240 chars of actual prose. **Graduated top-1:** the
  first hit gets a 400-char budget when its cos ≥ 0.55 (usefulness delivered inline beats the
  fetch loop's friction; an out-of-domain top-1 stays at 240 — no noise injection).
- **`type` and `kind` are hardcoded literals `"memory"`**, not schema columns (there is no `type`/`kind`
  column — §4.2). They're placeholders for the `[Target]` typed/kinded model.
- **`source_path` is NOT returned** (no such column). `[Target]`.

If the service is down, Windows hooks ask Task Scheduler to start the single scheduler-owned daemon;
other platforms retain a direct-spawn fallback. Writes that still fail enter the bounded durable
outbox — fail-open, never blocks a tool.

---

## 5. The Skill Output Contract + `skill_emit`

Full classification of all 67 skills: `tools/lib/SKILL-OUTPUT-CONTRACT.md`. The essence:

### 5.1 Three buckets

| Bucket | Skill kind | Durable output | Goes to |
|---|---|---|---|
| **A — Knowledge** | blueprint, audit, architect, seo, product-marketing-context, … | facts an agent should recall later | **OKF bundle → engine** (via `skill_emit`) + ONE human `.md` |
| **B — Content** | blogs, copywriting, design, … | a venture deliverable | the venture's content path, NOT the engine |
| **C — Ephemeral** | research, brand-identity, plan, growth, brief, … | nothing durable (answer is inline) | nothing persisted |

Research/brand-identity/plan/growth return their answer inline and persist no durable doc, so they
are bucket-C with nothing to migrate — by design, not omission.

### 5.2 `skill_emit` — the one durable path

`tools/lib/skill_emit.py`. A bucket-A skill's only new responsibility: build a `concepts` list and
call the wrapper. It writes the OKF bundle in-process (`okf.emit_bundle`, no subprocess, no Node)
then best-effort ingests each concept into the running engine:

```python
concept = {"name","type","title"?,"description"?,"tags"?,"body","links"?}

emit_knowledge(concepts, out_dir, compress=False)   # -> OKF bundle at out_dir, then memright put --file each .md
```

Helpers + CLI:

```
blueprint_concepts(repo)      # .agent/understanding.json dimensions -> 5 concepts
report_concepts(md,type,slug) # split a markdown report: preamble+H1 = overview, each "## " = a concept
emit_blueprint(repo)          # understanding.json -> OKF bundle + ingest
emit_report(report_path,type,repo)

py -3.11 tools/lib/skill_emit.py blueprint <repo>
py -3.11 tools/lib/skill_emit.py report <report.md> --type <audit|seo|design|context|...> --repo <repo>
```

`compress` defaults OFF — concepts are already distilled, and compression loads a heavy LLMLingua
model; opt in only for verbose bodies.

### 5.3 What each wired bucket-A skill emits

| Skill | Trigger | Emits (`type`) |
|---|---|---|
| `blueprint` | mandatory `skill_emit blueprint <repo>` at Phase-2 close | architecture, interfaces, health, security, solid |
| `audit` | synthesis step | audit (overview + one concept per `##` finding) |
| `architect` | plan synthesis | design (the ADR + plan as concepts) |
| `seo` | `*full-audit-report.md` write (auto-routed) | seo |
| `product-marketing-context` | `.agents/*product-marketing*.md` write (auto-routed) | context |

---

## 6. The hooks (recall + write-through + enforcement)

The live recall adapters and write-through hooks are repo-carried. Claude hooks are installed under
`~/.claude/hooks/` and wired in `~/.claude/settings.json`; Codex recall is supplied by the local brief
plugin. All recall paths fail open to the legacy semantic implementation.

| Hook | Event | Matcher | Does |
|---|---|---|---|
| `recall_planner.py` | UserPromptSubmit (Claude) | `*` | requests the authenticated federation packet, verifies provenance seals, delivers bounded context, and invokes `recall_legacy.py` on off/shadow/unavailable/degraded paths. |
| `tools/codex-brief-plugin/recall_planner.js` | UserPromptSubmit (Codex) | `*` | provides the same on-mode federation, seal, delivery, and legacy-fallback contract through `brief@local-brief` 1.0.4. |
| `recall_memory.py` via `recall_legacy.py` | fallback only | — | performs bounded semantic `/recall`, session dedupe, attribution/heartbeat logging, and fail-open service recovery when the planners select the legacy path. |
| `ingest_memory.py` | PostToolUse | `Write\|Edit\|MultiEdit` | (1) **enforcement:** known knowledge-doc writes auto-route through `skill_emit report`; (2) memory `/memory/`, blueprint `/.agent/okf/`, and `START-HERE.md` writes use **`memright put --file <path>`**. Nonzero subprocess exits are failures. Failed writes enter an atomic durable outbox with a stable id; at most one item is replayed per hook run and retries are rate-limited, so outage recovery is bounded and fail-open. |
| `normalize_memory_frontmatter.py` | PostToolUse | `Write` | normalizes YAML frontmatter on memory writes; startup dependencies are covered by a subprocess smoke test. |
| `recall_rearm.py` | SessionStart | `compact` | deletes the session's recall-seen file after compaction so memories wiped from context may re-inject (post-compaction re-injection, added 2026-07-04; supersedes the old "stay marked seen" trade-off). |

**Test coverage:** `py -3.11 -m pytest tools/hooks -q` covers planner fallback, attribution,
low-signal filtering, Unicode tokenization, scheduler-first service startup, normalizer startup,
nonzero ingest exits, atomic outbox writes, retry throttling, and scoped path routing. Installed
Claude hooks are reconciled from the repo by `setup-workspace.py`. **Hygiene code (8 constants + 3
helpers) is still duplicated** between
`recall_memory.py` and `recall-relevance-spotcheck.py` — `tools/lib/memory/recall_hygiene.py` is the
queued refactor; the drift risk is real (one bug already had to be fixed in one file after the
other had drifted), but no new rule has been added since 2026-07-09 so the duplication is stable.

**Enforcement-by-mechanism (best-effort, not unbypassable).** When `ingest_memory.py` is installed and
runs, it auto-routes these write paths through `skill_emit` so the OKF emit happens even if a skill or
agent forgets its SKILL.md step:

```
docs/plans/*.md                 -> skill_emit report --type design
.agents/*product-marketing*.md  -> skill_emit report --type context
*full-audit-report.md           -> skill_emit report --type seo
.audit/*  or  *-audit-report.md -> skill_emit report --type audit
```

This narrows two failure modes **when the hook fires**: knowledge is unlikely to be stranded outside the
engine (auto-emit), and sprawl is kept out of recall (only `okf/` + `/memory/` are ingested directly —
`runs/` and other working files are refused). **It is not a hard guarantee:** the hooks are fail-open
(§ above) and per-machine config (§13) — an absent, disabled, or erroring hook silently routes nothing.
So this is *best-effort enforcement that defaults to safe*, not a mechanism that's impossible to bypass.

**`[Live]` (2026-07-02) — layers 1 + 4 are enforced-by-hook, not prompt text.** The directive *strings*
live in `tools/lib/policy.toml` (single source; each hook carries a byte-identical inline fallback, so a
missing/broken policy file is fail-soft, never a behavior change):
- **L4 (machine-minimal)**: the outbound spawn hook mutates `tool_input.prompt` (that IS enforcement),
  reads `DIRECTIVE` from policy.toml, and lints the task body against `prose_markers` — logging
  `lint_prose_markers` + `prompt_chars` per spawn to `agent-spawn-audit.log`
  (`lib/machine_minimal.py::lint_prose_markers`). Warn-only → deny only if the data shows repeat
  regressions.
- **L1 (brief)**: `enforce_brief.py` emits the directive from policy.toml, and the Stop hook
  `brief_meter.py` (repo copy `tools/hooks/brief_meter.py`, installed by `setup-workspace.py`, wired in
  `settings.json` Stop) appends one JSONL line per turn to `tools/.cache/metrics/brief-meter.jsonl`
  (`{ts, session_id, reply_chars, banned_opener}`) — L1's first measurement. Warn-only: it NEVER blocks
  a stop; a bad blocker on the reply path is worse than verbosity.

`settings.json` wiring (the relevant entries):

```json
"UserPromptSubmit": [{ "matcher": "*", "hooks": [{ "type": "command",
  "command": "py -3.11 C:/Users/adrds/.claude/hooks/recall_planner.py" }] }],
"PostToolUse": [
  { "matcher": "Write", "hooks": [{ "type": "command",
    "command": "python C:/Users/adrds/.claude/hooks/normalize_memory_frontmatter.py" }] },
  { "matcher": "Write|Edit|MultiEdit", "hooks": [{ "type": "command",
    "command": "py -3.11 C:/Users/adrds/.claude/hooks/ingest_memory.py" }] }
]
```

---

## 7. End-to-end flow

```
                          ┌─[8 mem-recency / curate]─ curates the durable store (decay/merge/prune/digest)
                          ▼
   durable store ──[7 recall hook]──> injects skel-ranked memory/blueprint ─┐
                                                                            ├─> my context / agent context
Adrian ──prompt──> me                                                      │
   me runs commands ───────────────> [2 runc] ───────────────────────────>┤
   me/skill fans out:                                                      │
       inputs ──[3 prep-context]──> agents   (code skel'd, prose okf'd) ──>┤
       spawn  ──[4 machine-minimal]──> agents (terse A2A)                  │
   skill emits knowledge ──[5 skill_emit → okf]──> bundle ──[ingest hook]──┘──> durable store (feeds 7/8 next time)
   me writes a knowledge doc ──[ingest hook auto-routes]──> skill_emit ──────> durable store  (enforcement)
   my context fills ───────────────> [6 /compact]
   my reply ──[1 brief]──> Adrian
```

PUSH (1–6) shrinks the transient; PULL (7) decides what's pulled in; PERSIST (8) decides what's kept.
The ingest hook is the closed loop: a durable write becomes recallable with **no migration lag**.

---

## 8. Setup from scratch

Standing up the whole engine + hooks on a fresh machine. Canonical engine source is the Rust
**`memright`** workspace at `tools/memright/`; the hooks are Python glue.

**1. Build the engine** (Rust toolchain). The `fastembed` feature loads onnxruntime at runtime via
`load-dynamic`, so it builds on any MSVC toolset:

```bash
cd tools/memright && cargo build -p memright --release --features fastembed --bins
cp target/release/memright(.exe) <stable path, e.g. D:/Claude/tools/bin/memright.exe>
# Windows also requires the scheduler-owned service binary:
cp target/release/memright-service.exe D:/Claude/tools/bin/memright-service.exe
```

**2. Provide an onnxruntime DLL/lib** and point `ORT_DYLIB_PATH` at it. Any compatible `onnxruntime`
(the Python `onnxruntime` wheel ships one at `…/site-packages/onnxruntime/capi/onnxruntime.dll`); for a
real install, bundle a matching `onnxruntime` shared lib with the binary. `fastembed` pulls
EmbeddingGemma-300M-Q4 on first use, then runs offline.

**3. Migrate the corpus** (set `ORT_DYLIB_PATH`, `MEMRIGHT_DB`, `WORKSPACE_ROOT` — `MEMRIGHT_DB`
is MANDATORY since 2026-07-05; the binary errors without it or `--db`):

```bash
memright migrate             # ~/.claude/projects/*/memory + global
memright migrate-blueprint   # <WORKSPACE_ROOT>/*/.agent/okf bundles
```

**4. Install and start the resident service** (loads the embedder once; keep it running). On Windows,
`install-windows-tasks.ps1` registers scheduler-owned `memright-serve` and `memright-daily`.
`memright-serve` executes the no-console `tools/bin/memright-service.exe` directly with the workspace
as its working directory; that binary pins DB/token/model/ORT runtime paths and launches the resident
serve path. The task runs at logon, restarts on failure, has no execution time limit, and is the only
normal owner of the daemon. Hooks request the scheduled task rather than spawning competing daemons:

```bash
powershell -File tools/pipelines/memory/install-windows-tasks.ps1
schtasks /Run /TN memright-serve
```

**5. Wire the hooks** — copy `recall_planner.py`, `recall_legacy.py`, `recall_memory.py`,
`ingest_memory.py`, and
`normalize_memory_frontmatter.py` into `~/.claude/hooks/`, add the §6 `settings.json` entries, and set
`MEMRIGHT_BIN` / `MEMRIGHT_DB` / `ORT_DYLIB_PATH` (the hooks default these to this machine's paths,
env-overridable). The planner owns RightContext federation and restores legacy recall on degraded,
stale, unavailable, or disabled paths. The hooks lazy-start the service.

**6. Verify** end to end:

```bash
curl -s http://127.0.0.1:47851/health                     # public liveness only
curl -s -X POST http://127.0.0.1:47851/recall \
  -H "authorization: Bearer $MEMRIGHT_API_TOKEN" \
  -H 'content-type: application/json' \
  -d '{"query":"how does recall scoring work","k":3,"scope":"D--Claude","observe":false,"traffic_class":"evaluation"}'
```

A non-empty, semantically-relevant `/recall` result (cos ~0.6+, not near zero) confirms the embedder,
the DB, the scope chain, and the service are all live. (cos near zero or garbage = the binary fell back
off EmbeddingGemma — check `ORT_DYLIB_PATH`; the build is set to fail loud, not silently hash.)

---

## 9. Operations

| Task | Command |
|---|---|
| Re-embed everything after a model or scoring change | `memright reindex` — in-place, memoized: skips rows whose `(content_hash, embed_model)` match (§10, ~4 s on an unchanged corpus) |
| Add a single new file immediately | write it and let the ingest hook run; or call `memright put <name> --scope <scope> --file <path>` |
| Restart after rebuilding | stop `memright-serve`, back up and replace both `memright.exe` and `memright-service.exe`, start the task, then verify `/health` |
| Roll back to a v9 binary | stop the service, back up the DB, run `memright --db <path> backout-schema-v10`, then replace both binaries and restart |

The service holds the embedder + DB in memory — **edits to the engine require a service restart** to take
effect for live recall/ingest. The DB itself is shared, so a direct CLI write is visible to the
running service immediately (committed rows), but new *code paths* are not.

**Recall preview compression is measured; net savings are not yet causal.** Every observed `/recall`
logs full returned-content chars vs injected-preview chars. Their difference is exposed as
`potential_chars_saved` / `potential_tokens_saved_est`: it measures avoided eager content injection,
not how many characters the model
would actually have fetched or needed without recall. A second outcome-linked field is still needed
for measured net savings — see §10.

**Daily analysis + curation are scheduled (2026-07-11):** after mirror pull/curation,
`daily-sync.sh` extracts up to 400 distinct real user queries from local Codex/Claude session history
into a private, hashed-provenance manifest, then runs `daily-analysis.py`. The analyzer joins
content-free MemRight lifecycle/policy rows to provider-observed Codex and Claude token telemetry,
writes a detailed local JSON/Markdown report, and publishes only an aggregate allow-list to the
protected `spoares.com/memory` dashboard. The hosted dashboard includes an interactive force-directed
memory graph whose nodes/edges preserve real relationship shape but contain no IDs, scopes, keywords,
paths, or content; the loopback graph uses real IDs/scopes and click-to-open content beside private
CRUD data. Neither graph replaces the measured tables. Binary, curation, replication, or
push failure makes the run fail; metrics refresh is still attempted for observability. Manual
invocation still works.
Per-memory put/update/inject/get/delete/curate timestamps and writer/access surfaces are live in the
v10 schema's lifecycle ledger (introduced in v6). Explicit task-success/benefit remains open; a full fetch is a mechanical
use signal, not proof of task quality.

**Semantic evidence release deployed 2026-07-11:** MemRight source `7eb79a9` (tree digest
`ea387a79…`) is installed as the manifest-hashed Windows asset `0221e0ce…`; CodeRight `main`
pins both `memright` and `memright-core` to that exact source. The local and hosted dashboards were
freshly inspected after deployment, and the hosted snapshot passed exact read-back plus content/ID
privacy checks. That legacy cohort design is now paused/baseline-only because on-mode prompt adapters
do not share its assignment point. Any future promotion still requires a new valid experiment with
at least 20 control plus 20 candidate clean sessions per comparable group, a 95% lower confidence
bound of at least 40%, and quality non-inferiority.

---

## 10. Measurement (the Graphify guard)

Graphify was deleted because it was write-heavy and read-zero — elaborate scaffolding nobody queried.
This engine is built to prove it's the opposite, before it's trusted or productized. Three distinct
measurements must not be conflated:

- **Potential-savings measurement — closed; net-savings measurement — open.** Every observed
  `/recall` records `full_chars` (complete content behind returned hits) vs `injected_chars` (the
  preview actually sent). `memright metrics` sums the log:
  ```
  MEMRIGHT_DB=<path> memright metrics
  {"recalls":2,"hits":10,"full_chars":37662,"injected_chars":1207,
   "potential_chars_saved":36455,"potential_tokens_saved_est":9113,
   "chars_saved":36455,"tokens_saved_est":9113,"since":"...","through":"..."}
  ```
  `tokens_saved_est` is chars÷4 (no tokenizer dependency, intentionally approximate). Treat these
  compatibility aliases `chars_saved`/`tokens_saved_est` as **potential** savings only: the
  calculation does not observe the model's
  counterfactual context or its eventual full-content fetches. Add an outcome-linked effective-tail
  metric before making a causal token-cost claim. Layers 1–5 have separate instrumentation; L6
  remains harness-owned and unmeasured.
- **Layer 2/3/5/8 instrumentation — `[Live]` (shipped 2026-07-02, Task 8a).** The transforms log to the
  same DB as recall: `transform_log(ts, verb, scope, before_chars, after_chars, meta)` written by every
  `skel`/`compress`/`prep`/`runc` invocation (CLI fail-open + serve routes; `curate` logs merge/prune
  counts), aggregated by `memright metrics` into a per-verb `transforms` block + a `curate` block.
  Verified live: one `skel` run logged 4249→184 chars; cutover smoke recorded `skel` 5 runs /
  38,008 chars saved. **L1 + L4 also `[Live]` 2026-07-02:** L4 logs `prompt_chars` +
  `lint_prose_markers` per spawn to `agent-spawn-audit.log`; L1 logs reply chars per turn via the
  `brief_meter` Stop hook (§6). L6 stays harness-owned (the one unmeasured layer, plus §10.1
  candidates). One DB, one `metrics` report — "is each layer worth it" is a query, not a claim.
- **Provider-token causal measurement — paused/baseline-only for on mode (corrected 2026-07-16).**
  The persisted `POST /policy/assign` cohort assignment currently runs only inside the legacy
  `recall_memory.py` fallback. The live on-mode planners do not assign cohorts at the shared planner
  entry, so those rows cannot support a causal RightContext-on comparison. `daily-analysis.py` may
  retain the legacy instrumentation, but `measured_reduction_pct` must remain null and status must
  remain `baseline_only`/paused until assignment covers the shared planner path or a new experiment
  is defined. Recall-payload compression is not a substitute for this KPI.
- **Autonomous promotion evidence — relevance corpus and four-arm tournament complete
  (2026-07-11).** The evidence pipeline collected 400 balanced real local queries and produced 200
  independently reviewed relevance rows (155 tune, 45 locked holdout) with immutable snapshot and
  label hashes. Production relevance rows
  may carry `human`, `agent-reviewed`, or `behavioral` provenance, but agent evidence must name the
  immutable query-source hash, rubric, and a candidate pool contributed by at least two retrieval
  arms. `model-proposal`, placeholder, synthetic, single-arm/circular, or snapshot-mismatched rows
  cannot promote. The exact production EmbeddingGemma Q4 model evaluated whole-document,
  contextual-768, fixed-512, and sentence-boundary arms with paired bootstrap confidence intervals.
  Whole-document retrieval won on tune and was retained after the locked holdout; no chunk table or
  chunking policy is promoted. `MEMRIGHT_LEARNING_POLICY=control` remains the global fail-closed
  recall kill switch.
- **Usage/effectiveness loop — CLOSED mechanically (shipped 2026-07-02, Task 8b); DATA still
  accumulating.** `/recall` persists `inject_count` per returned hit; `POST /get {id}` /
  `memright get <id>` returns full content and records use; the recall hook injects the exact get
  command with every block. Effectiveness = `(access+1)/(inject+2)` (§4.6). Verified live on the real
  DB (injected+fetched row at 1/1, injected-never-fetched at 0/1). What remains open is the DATA —
  review the fetched-after-inject rate via `/context-metrics` against the §10.2 thresholds before
  wiring effectiveness into the recall sort.
- **Paired-metric protocol (2026-07-05).** `memright metrics` now emits an `effectiveness` block
  (`corpus` / `injected_distinct` / `fetched_after_inject` / `rate`, serve rows only — CLI recalls
  log `source='cli'` and never bump `inject_count`). The fetch-after-inject rate measured **2.9%**
  (14/483) on 2026-07-10 — but it is a confounded LOWER BOUND: preview sufficiency and
  Bash-fetch friction both suppress it, and the graduated top-1 preview suppresses it further by
  design. So the rate is ADVISORY; a §10.2 kill/clear decision requires the PAIR — rate plus a
  20-query blind relevance spot-check on real logged queries (`query_preview`) at each
  `/context-metrics` review.

- **Recall-attribution audit (`c0b6045`, 2026-07-10).** `tools/pipelines/memory/recall-attribution-audit.py`
  classifies `client='unknown'` rows into shape buckets (`empty_query` / `legacy_no_preview` /
  `task_notification` / `divider` / `normal`) and cross-references each unknown `session_id`
  against `memright-heartbeat.jsonl` — many "unknown" rows from pre-deploy sessions can be
  **inferred** to be `claude` or `claudemm` based on heartbeat evidence (the heartbeat is
  post-deploy, the DB row is pre-deploy). Read-only by default; `--archive-unknown --apply`
  moves them out of active `recall_log` into a sidecar `recall_log_unknown_archive`
  table with `archived_at` + `archive_reason='legacy_unattributed_recall_telemetry'`. Run after
  any deploy that touches attribution: it cleans the historic 1100+ unknown rows that diluted
  the per-client cross-client audit. Current state after applying (2026-07-10 22:41Z):
  `counts_by_source_client` shows 6 distinct clients all properly attributed; `unknown_by_day: []`.
- **20-query relevance spot-check — `[Live]` (hardened 2026-07-10).**
  `tools/pipelines/memory/recall-relevance-spotcheck.py` samples recent qualifying production
  `source=serve` rows, replays `/recall` with `observe=false, traffic_class=evaluation`, and judges
  each row `relevant` / `partial` /
  `irrelevant` against the full memory content (not just the 200-char skel — `judge_basis:
  full_content_when_available_preview_fallback`). Calibrated thresholds after the
  2026-07-09 round-1 audit: `COS_RELEVANT_AUTO = 0.45` (was 0.55 — sat at the wrong percentile
  of the natural 768-dim cos distribution; `strict_rate` jumped 7.7% → 32% → 54% as the threshold
  was tuned), `COS_PARTIAL_AUTO = 0.35` (was 0.40). The hook's `MIN_COS = 0.40` injection floor
  remains at 0.40. Latest run: 13/10/1 R/P/I over 24 evaluated samples; `relevance_rate = 0.54`,
  `useful_rate = 0.96`, `recommended_action = "keep"`, `confidence = "earning"`. **Single-judge
  only** — `variance_status = "not_measured_single_judge"` is honest about the gap; the
  `/context-metrics` skill's table flags it as "do not treat variance as zero". Shadow
  rankers (BM25, cosine-proxy, hybrid) distinguish `novel_candidates` (any different
  candidate) from `would_have_helped` (different candidate that passes the relevance proxy) — a
  clear gate for any future reranker proposal. The run is measurement-only: it adds no recall rows
  and increments no injection counters. Fetch failure, an unreadable DB, or zero qualifying samples
  exits nonzero and emits an atomic failed report instead of producing a false healthy verdict.
- **Reranker-gate data EXISTS now (2026-07-05):** the fused-order ranking experiment was gated,
  measured against a frozen snapshot, and REVERTED (+3.70 mean-rank degradation on known-useful
  targets — §4.6). Any cross-encoder/reranker work now has a baseline and a burden of proof:
  win on a real-query replay first.
- **Measure first, expand second** remains the right policy for what's still open — do not widen the
  corpus (transcripts) or productize further until usage/effectiveness is closed too. The same gate
  applies to the **LLMLingua-ONNX deploy (709 MB)**: heuristic compress ships by default; the ONNX
  assets deploy only after `transform_log` shows L3-compress volume + savings that justify the weight.

### 10.1 Coverage audit (2026-07-02) — what the 8 layers still don't touch

Two token flows have NO layer, and both get **measurement before mechanism**:

- **Main-session tool results (Read / Grep / WebFetch dumps)** — likely the single largest transient
  flow, and harness-owned: interception was deliberately rejected with the rest of Headroom's network
  layer, so the only levers are read discipline (policy, already in brief-mode) and **measurement**.
  `[Target]` candidate: a PostToolUse **context ledger** hook logging `{tool, result_chars}` per call —
  a week of data says whether any mechanism here is worth building. Do not build a mechanism first.
- **Session boot static load** — CLAUDE.md + six @-imported rule files + MEMORY.md inject every session,
  before the first prompt. MEMORY.md's one-line index partially double-covers what the recall hook now
  injects semantically. `[Target]` candidate: measure boot chars once (trivial: sum the injected files),
  then diet — move more rules to the on-demand pattern (§ CLAUDE.md already does this for vast/remotion/
  data-extraction) and consider shrinking MEMORY.md to pointers the recall hook supersedes. Measure, then
  cut; no new machinery.

**Prompt-cache discipline (added 2026-07-02).** Anthropic's prompt cache (cached-read tokens ~10%
price, ~5-min TTL) is the OTHER half of the token economy: it discounts REPEATED tokens; this stack
reduces tokens outright — orthogonal and multiplicative. Two standing rules: (1) hook injections must
stay APPENDED to the newest message (they are — `additionalContext` rides the new user turn), never
prepended before stable content, or they break prefix stability and forfeit the cache; (2) every
CLAUDE.md / rules-file edit invalidates the cached system prefix for ALL sessions on the machine —
batch doc edits instead of dribbling them, and the §10.1 boot-diet doubles as cache-write reduction.
Engine-side, embeddings are memoized (2026-07-02): rows carry `(content_hash, embed_model)` and
`memright reindex` skips unchanged rows — full reindex dropped from ~25 min to ~4 s (verified:
`{"reindexed":0,"skipped":598}`), making embedder swaps/repairs cheap.

**Retrieval-quality research is gated, not scheduled:** a cross-encoder reranker over the cosine top-k
(the standard next rung) and query expansion are worth it ONLY if Task-8b effectiveness data shows
relevant memories exist but rank below the cutoff. Don't add model weight to fix a problem the data
hasn't demonstrated.

**Future-look: transform memoization (parked 2026-07-02, decision due at the first monthly digest
with ≥30 days of transform_log).** Caching `skel`/`prep`/`compress` outputs keyed by
`(file_hash, rate)` was considered alongside embedding memoization and deliberately NOT built —
current data shows single-digit runs of millisecond-fast transforms. **Promotion criterion** (check at
the `/context-metrics` review): ≥1,000 transform runs in the trailing 30 days, OR any single prep
fan-out over ~200 files, OR the LLMLingua-ONNX compressor deploys (slow enough per-call to change the
math). If none of those fire in the 30-day window, note it and re-park; if it never fires in 90 days,
delete this item.

### 10.2 Kill criteria (the anti-Graphify contract, concrete)

Graphify died because "is anyone using this" was never asked. Here it is asked on a clock — run
**`/context-metrics`** (skill at `tools/skills/context-metrics/`, added 2026-07-02: fresh
`memright metrics` + hook logs → per-row verdicts + ONE recommended action), standalone anytime and
inside the monthly `/mem-recency` digest, against these thresholds:

| Signal (after 30 days of data) | Action |
|---|---|---|
| fetch-after-inject rate low (ADVISORY — it's a confounded lower bound) AND the 20-query blind relevance spot-check says injections are irrelevant | raise `MIN_COS`, lower k, or fix the preview — only the PAIR can kill or clear (2026-07-05 protocol; the rate alone measured 2.9% on 2026-07-10 while the calibrated spotcheck reports 0.54 strict + 0.96 useful) |
| `strict_rate` from the relevance spot-check < 0.4 AND `useful_rate` > 0.7 | tune the auto-judge threshold OR look for retrieval-ranking fixes (NOT a kill candidate on its own — single-judge variance is unmeasured) |
| `useful_rate` < 0.4 AND `shadow.would_have_helped` total > 2 | ranking is the suspect — investigate shadow alternatives before touching the hot path |
| a transform verb with zero `transform_log` rows | delete the verb (and its serve route) — it's Graphify |
| `/skel` `/prep` serve routes still consumer-less post-cutover | ~~remove the routes~~ **DONE 2026-07-05** — first kill-criterion row to fire and execute |
| a `quarantined_scope` event in the heal ledger older than 30 days | fold the dir with `reconcile-scope.py` or extend the sync canonicalizer — quarantines must not rot (the `heardright` mirror scope is quarantined as of 2026-07-10) |
| repeated `killed_wedged_serve` heal events | the serve wedge cause is persistent — debug the engine, don't rely on the watchdog |
| brief-meter / spawn-lint logs that never change behavior | delete the hook — measurement that informs nothing is cost |
| recall hook injecting 0 survivors on most prompts | the corpus or threshold is wrong — fix or shrink, don't expand |

The system's usage is structural (the recall hook fires on every prompt; transforms sit inside existing
call paths), which is the opposite of Graphify's opt-in-that-nobody-opted-into — but structural usage
only proves the machinery RUNS. The table above is what proves it EARNS.

---

## 11. Hard exception — VERIFY/EDIT reads are never compacted

You cannot confirm or change exact logic against a skeleton or token-dropped doc. Compaction (layer
3 / `skel` / `compress`) applies only at SURVEY / orientation / synthesis reads. Blueprint Phase 2a
(claim verification) and any edit-intent read pull **full** on purpose. Recall injects `skel` for
orientation; the agent reads the full `source_path` before editing.

---

## 12. CodeRight mirror + the unified engine

**Layers 7 + 8 (retrieval + curation) are no longer "CR vs workspace, converging" — they are ONE
engine.** As of 2026-07-01 the `memright` crate is the extraction of CR's mature, persisted, governed
memory store (tiers/retriever/routing/effectiveness/dream/quantized-persistence) out of the
`coderight` product binary. Canonical source ownership is now `tools/memright/` in this workspace;
CodeRight's duplicate legacy source was removed and it consumes exact git-revision pins for
`memright` and `memright-core`. **`[Live]` (reviewed 2026-07-11):** CodeRight's daemon links `memright`,
opens `memright::MemoryStore` over `~/.coderight/evolution.db` (`main_runtime_server.rs`), and
injects `state.memory.context_for(...)` on the direct-message route + sub-agent briefings
(`api_sessions.rs`, `daemon_state.rs`). **Shipped 2026-07-04 (CE-3 core):** a turn-0
`<workspace_memory>` system-prompt block (`session_memory_block`, retrieved at session creation) and
the model-facing **`memory_save`** write tool (`memory_save_tool.rs`, bin-registered with the daemon's
store handle, honors allow/deny filters). **`[Partial]` remaining gap:** the per-turn engine-loop seam
still hardcodes memory to None (`api_state_impl_04.rs`) and post-compact re-injection is unbuilt
(blocked on CE-2's PostCompact hook) — CE-3 remainder in
`coderight/docs/plans/CONTEXT-ENGINEERING-UPGRADE-2026-07-04.md`. The workspace runs the canonical crate over its own
`tools/.cache/memory/memright-engine.db`. No mixing of data, no fork of code. CR is a *consumer* of
`memright`; this workspace repo owns the distributable source and verified release manifest (see §13).
Plan-of-record: `coderight/docs/plans/2026-06-30-memory-engine-unification.md`.

For layers 1–6 (compaction), CR still enforces some at the runtime seam where the workspace can only
instruct via skills — that asymmetry is real and unrelated to the memory engine:

| Family | Workspace layer | CR equivalent | Status in CR |
|---|---|---|---|
| compaction | 1 `brief` | system-default terse prompt | specced WS-1 |
| compaction | 2 `runc` | `memright runc` (exec + cap + spill) | **unified — live** (workspace call-sites flipped 2026-07-02) |
| compaction | 3 `prep-context` | `memright skel` + `memright prep` (tree-sitter; 4-branch manifest parity) | **unified — live** (workspace call-sites flipped 2026-07-02) |
| compaction | 4 machine-minimal | conductor→agent scoped brief | WS-5 (handoff); workspace side → `policy.toml` + spawn-body lint (Task 7 STEP 3) |
| compaction | 5 `okf`/`skill_emit` | `memright compress` + `config::okf` (lib reuse) | **unified — live** (workspace call-sites flipped 2026-07-02) |
| compaction | 6 `/compact` | `compactor.rs` (transcript) | present |
| **retrieval** | 7 recall | `memright::store` (hybrid candidates via `coderight-memory`'s retriever + `quant.rs`; NOTE 2026-07-05: `graph.rs` (MemoryGraph) has ZERO production callers — it is the [Target] links-graph primitive kept deliberately, and `routing.rs` is used only inside `context_for`, the CR-daemon-only path) | **unified — same crate as workspace** |
| **curation** | 8 `mem-recency`/dream | `memright curate` → `dream_now` (in-place consolidation) + `effectiveness.rs` | **CLI verb merged 2026-07-02; scheduled in `daily-sync.sh` + dream reshaped in-place 2026-07-04**; effectiveness loop open until Task 8b (§4.6) |

---

## 13. Portability & deployment status (honest)

**The engine core (`memright` crate) is OS-agnostic Rust** — SQLite (`rusqlite`, bundled), fastembed/
onnxruntime (`load-dynamic`, no MSVC-static-link dependency), and the rest of `coderight-memory` are
all cross-platform. `skill_emit.py`/`okf.py` (the hooks' Python glue around it) are pure Python +
`pathlib` — no OS-specific calls, no hardcoded separators. They run on Windows, macOS, and Linux
unchanged.

**The hooks are OS-portable AND repo-carried (since 2026-07-02).** The Claude planner and fallback
(`recall_planner.py`, `recall_legacy.py`, `recall_memory.py`) plus `ingest_memory.py` and
`normalize_memory_frontmatter.py` live canonically in `tools/hooks/`; the Codex adapter lives in
`tools/codex-brief-plugin/`. `tools/setup-workspace.py` installs the Claude hooks into
`~/.claude/hooks/` and registers their `settings.json` entries with the right launcher (`py -3.11` on
Windows, `python3` elsewhere). Portability details:
- Detached-spawn is `os.name`-guarded (`DETACHED_PROCESS` vs `start_new_session`).
- Engine paths (`MEMRIGHT_BIN`/`MEMRIGHT_DB`/`ORT_DYLIB_PATH`/`HF_HOME`) default os-conditionally off
  the workspace root (`D:/Claude` vs `~/claude`) — env vars override. On macOS `ORT_DYLIB_PATH` should
  point at a `libonnxruntime.dylib` (default expects it at `tools/bin/`).
- `WORKSPACE_SKILL_EMIT` still defaults to the Windows path in `ingest_memory.py` — set it on mac.

**Current deployment boundary.** The production-hook cutover, authenticated service, and v10 engine
state were validated on Windows on 2026-07-16. Both machines use repo-carried setup and daily-sync
wiring, but Mac-native parity should be revalidated with `docs/CODEX-MAC-MEMRIGHT-HANDOFF.md` after
the next Mac setup/pull; Windows evidence must not be presented as Mac-native proof.

**CodeRight's memory engine (layers 7–8) IS this stack natively — one codebase (§12).** The
deterministic compaction layers (2/3/5) **merged into the same crate 2026-07-02** (unification plan)
**and the workspace call-sites flipped to the `memright` shims the same day** — the old Python/Node
copies (`runc.mjs`, `prep-context.py`, `skel.py`/`.mjs`, `compress.py`, `okf.py`'s compress path) are
retained one release as rollback, not the live path. What remains divergent is layers 1/4 (unifying as
`policy.toml` + enforcement hooks, Task 7 STEP 3, landing in this same change) and layer 6 (stays
edge-split — the LLM call can't be shared). CR *the app* is cross-platform (Rust + Tauri, per
`RIGHT-SUITE-CROSS-PLATFORM.md`); the memory engine it ships is the same `memright` crate the
workspace runs, not a parallel reimplementation.

**Deployment caveat:** the Mac has not run the 2026-07-10 cutover — see the Windows-only paragraph
above.

## 13a. Cross-machine sync — immutable events through git, DBs rebuilt locally **`[Live]` replication v2, 2026-07-10.**

Each machine's DB is authoritative for local embeddings and telemetry; durable CONTENT mutations sync
through the private workspace repo as immutable, content-addressed JSON events:

- `memory-mirror/_events/<injective-origin>/<event-id>.json` is append-only. Put and delete events
  carry canonical id, mutation time, origin, and provenance. Strict parsing rejects malformed JSON,
  non-finite/non-positive timestamps, invalid identities, and path traversal.
- `memory-mirror/_replicas/<injective-replica-id>.json` is a single-writer progress record. Replica
  and origin filesystem keys include a hash suffix, so distinct machine identities cannot collide
  after sanitization. Legacy manifests remain read-only migration input.
- Apply is deterministic LWW over `(updated_at, delete-wins-at-equal-time, origin, event-id)`. A
  legacy record may fall back to its source time, but new mutations require explicit `updated_at`.
  Tombstones are permanent: there is no TTL or unsafe delete compaction. A later put wins only when
  its mutation clock is newer under the same total order.
- A push-only invocation first validates and reapplies all remote events, then exports local state.
  This removes the old stale-writer window where push-before-pull could overwrite a newer peer or
  erase its tombstone. Embeddings are always rebuilt locally through MemRight.

The daily job operates from a dedicated clean checkout, resolves both physical and git identities,
pulls with rebase, applies remote events, curates, exports, stages only `memory-mirror/`, and pushes.
It refuses untracked/dirty mirror state and preserves a clean unpushed commit for retry. The live
workspace's unrelated edits can neither block nor leak into the replication commit.

**Why the DB is NEVER the sync unit:** (1) scope ids are cwd-derived, so a copied DB's rows never
match the other machine's recall chain — silently deaf; (2) `recall_log`/`transform_log`/
`access_count`/`inject_count` are per-machine telemetry the kill criteria depend on — merging corrupts
the measurement; (3) a binary SQLite in git is unmergeable (last-write-wins data loss).

**What syncs vs what regenerates (the full corpus, not just memories):**
| Corpus | Sync mechanism |
|---|---|
| Memories (DB rows) | `sync.py` replication v2 (above): DB mutation → immutable event → deterministic apply → local embed — the one hand-curated, non-regenerable corpus |
| Blueprint OKF (`<repo>/.agent/okf`, gitignored) | NOT synced — derived from repo code; regenerate on the other machine (`blueprint` or `migrate-blueprint` if a bundle exists) |
| Skill emissions (audit/seo/design/context reports) | The report `.md`s are tracked in their repos and sync via git; each machine's ingest hook / `skill_emit report` re-ingests on write. After a big pull, `memright migrate-blueprint` re-ingests bundles present on disk |
| Engine DBs (`memright-engine.db`, CR `evolution.db`) + all telemetry | Per-machine forever — derived index + local measurement |

## 14. Reconcile (drift control)

- **Policy SoT:** this file. The families, the eight layers, the routing rule, the engine
  architecture, and the contract all live here.
- **On any change** to the routing rule, a layer definition, the schema, or the scoring formula:
  update this file first, then the CR map (`coderight/docs/CONTEXT-ENGINEERING.md`), the deep-
  integration spec, and `SKILL-OUTPUT-CONTRACT.md` in the same turn.
- **Drift check:** the CR mirror table must match these eight layers 1:1. A row there but not here
  (or a different routing rule) is drift to reconcile, not a new feature.
- **Doc truth check before saying "synced":** inspect the live Rust surfaces that have drifted before:
  `coderight/engine/crates/memright/src/memdb.rs` for §4.2 schema,
  `coderight/engine/crates/memright/src/serve.rs` for §4.8 response shapes,
  `coderight/engine/crates/memright/src/main.rs` for §4.7 CLI verbs, and
  `coderight/engine/crates/memright/src/store.rs` for §4.6 recall scoring / §8 curation wiring.
  If the code and this doc disagree, tag the doc claim `[Target]` or fix the code before repeating it.
- Renamed from `COMPACTION.md` 2026-06-30; referrers updated (`coderight/docs/`, CR `ROADMAP.md`).
- **ADR / decision record for the 2026-07-05 hardening pass** (worker-pool serve, recall watchdog,
  fail-loud DB, source-split + query_preview logging, real previews, cos-gated top-1, the T2.1
  fused-vs-cosine ranking experiment that was gated/measured/reverted, /skel+/prep route removal,
  boot diet, sync quarantine): [`docs/plans/2026-07-05-memright-context-engineering-next.md`](../../docs/plans/2026-07-05-memright-context-engineering-next.md)
  — the ADR block, the six-model external review synthesis, and the "T2.1 verification verdict" all
  live there. This doc is the living SoT; the plan is the frozen why-and-evidence for that change.
- **Audit-driven changes 2026-07-09 → 2026-07-10** (in temporal order):
  - `941e78ac feat(memright): attribute recalls by client` — added the `client` / `session_id` /
    `cwd_scope` / `hook_event` / `trace_id` / `client_visibility` columns to `recall_log` and the
    `attribution` block to `metrics_json`. Was the prerequisite for the cross-client audit (§10).
  - `28547be fix(memright): calibrate recall relevance spotcheck` (top-level) — added the
    20-query relevance spotcheck + Phase 3 hook hygiene (low-signal filter, mojibake divider
    detection, task-notification unwrap with extractable-inner check).
  - `533f836b fix(memright): soften scope-chain recall ordering` (coderight) — replaced the hard
    scope sort with `SCOPE_CHAIN_SOFT_BONUS = 0.02` per rank; tie → exact wins, clearly-better global
    → global wins. Test guard `recall_scored_uses_scope_as_soft_bonus_not_hard_sort` covers both.
  - `d0c34335 fix(memright): harden serve writes and routes` (coderight) — exact path matching
    (no `/puts` → `/put`), `json_body` helper (empty vs malformed distinct 400s), `try_service_post`
    returns `Result<Option<String>, String>` so 4xx/5xx propagates instead of silently
    splitting-brain. The biggest correctness fix in this chain.
  - `4edbd24c test(memright): cover soft scope recall tie behavior` (coderight) — strengthens the
    533f836b test to cover both directions (exact-on-tie, stronger-global-on-bigger-cos).
  - `f3e67ad fix(memright): deploy audit-calibrated spotcheck signals` (top-level) — calibration
    `0.55 → 0.45 / 0.40 → 0.35`, `variance_status = "not_measured_single_judge"`, shadow-help gating
    tightened (3+ token overlap for relevant, was 2+).
  - `4f6531b fix(memright): tighten spotcheck judge and sample filter` (top-level) — full-content
    overlap (was skel-only), `audit-smoke-test-XXX` filter gap closed, recommendation rule
    updated to "remeasure after post-deploy attributed traffic" rather than the stale "tune
    preview/judge".
  - `692c9b9 fix(memright): harden ingest and daily metrics` (top-level) — disabled `POST /add` in
    the engine (still 410s if hit), rewired `ingest_memory.py` to `memright put --file` via
    `_memory_name` and `_scope_for_path` helpers.
  - `0ce3b43 chore(memory): daily mirror sync (Bogus-Dell)` (top-level) — the actual daily mirror
    push that pulled the dashboard up after a 24h gap.
  - `60e2e6f fix(memright): make daily sync env-safe` (top-level) — `daily-sync.sh` now uses
    `${WORKSPACE_ROOT:-}` / `${PYBIN:-}` (was breaking under `set -u` when the scheduler
    environment didn't export these).
  - `c4fb78b1 fix(memright): clear audit lint drift` (coderight) — final clippy/memdb cleanup
    accompanying the d0c34335 changes.
  - `c0b6045 fix(memright): archive unknown recall telemetry` (top-level, 2026-07-10) — adds
    `tools/pipelines/memory/recall-attribution-audit.py`, which classifies pre-attribution
    `client='unknown'` rows by shape, cross-references them with `memright-heartbeat.jsonl`
    for inferred client, and (with `--archive-unknown --apply`) moves them out of active
    `recall_log` into `recall_log_unknown_archive`. This is what closed the final live-evidence
    gap: post-archive, every row in `recall_log` has a real `client` value (or empty for
    post-deploy, which `recall-attribution-audit.py` verifies on every run).
  - `e79c9984 fix(memright): harden architecture and scoped admission` (coderight, 2026-07-10) —
    transactional schema v5/mutations/curation; SQLite 3.53.2; Axum auth/origin/body/concurrency/
    deadline boundaries; observer-free evaluation + replay telemetry; strict F-17 fallback; and one
    sibling-isolated, budgeted, provenance-receipted CodeRight admission path across every runtime
    seam. Metrics now expose `potential_*` names while keeping legacy aliases.
  - `ab47ad7 fix(memright): harden hooks replication and service ops` (top-level, 2026-07-10) —
    durable bounded ingest outbox, fail-loud subprocess handling, Unicode spotcheck judge,
    replication-v2 immutable events, dedicated-checkout daily sync, scheduler-owned Windows service,
    token propagation, and 38 hook + 53 pipeline test coverage.
  - `063d6f3 chore(memory): daily mirror sync (Bogus-Dell)` (top-level, 2026-07-10) — first
    replication-v2 event export. It applied one remote memory, exported the 999-row local state,
    emitted 425 new events + 118 tombstones, and pushed with zero validation/storage errors.
  - **Live cutover proof (2026-07-10 01:32Z):** service task running and daily task enabled/ready;
    exact release embedder probe 768/768 nonzero; DB integrity `ok`, schema v5, 999 memories, zero
    unknown clients; unauthenticated metrics 401, hostile Origin 403, missing content type 415,
    oversized body 413; observer-free 20-query spotcheck left recalls/hits/inject counters exactly
    434/2152/5886 and reported 0.30 strict / 0.75 useful (`watch`); the second full daily cycle was
    byte/idempotent at the event layer (`pushed=0`, `pulled=0`, `conflicts=0`, `in_sync=1117`) and
    left remote HEAD unchanged. Online-backup evidence lives in gitignored
    `tools/.cache/memory/snapshots/20260710T013209Z-post/` (pre-fix baseline:
    `20260709T234104Z/`).
  - **Audit reference (read in full before any future change to scoring/recall/hygiene):**
    [`docs/plans/2026-07-09-memright-rag-gap-architecture.md`](../../docs/plans/2026-07-09-memright-rag-gap-architecture.md)
    + the M3 review packet at `docs/plans/2026-07-09-memright-m3-review-packet.md`. The audit
    caught the live-hook deployment gap, the split-brain race, the path-traversal surface, and
    the auto-judge threshold artifact; all four are now closed.
  - **Open structural debt** (real but not blocking): `store.rs` is 2314 lines mixing 7 concerns
    (embedder bootstrap, registry I/O, dream consolidation, recall, metrics, export, tests);
    the hygiene rules are duplicated across `recall_memory.py` and `recall-relevance-spotcheck.py`;
    long entries are embedded as a single model-truncated document rather than deterministic chunks;
    model identity is stored as a label rather than a cryptographic artifact fingerprint; and the
    permanent event/tombstone log has no proven-safe compaction protocol. None block the current
    path; all are queued behind evidence and migration tests.
