# The ambient injection loop — from "callable service" to "Claude's context"

**Status:** pending · two repairs landed locally (`091a7e61`, `ac63cf56`), not yet qualified on the installed product.
**Scope:** Pull (federate route, freshness), hook host transport (`membrane hook`), Cortex/Ledger/Blueprint provider budgets, Hub lifecycle.
**Problem owner:** any agent host (Claude Code first) that is supposed to receive Membrane context without asking for it.

---

## 1. What "final shape" means

Membrane is ambient when every user prompt in a registered repository arrives at the model
already carrying the fused packet, and nobody invoked a tool to make that happen. Concretely:

1. Claude Code fires `UserPromptSubmit`; `membrane hook` returns `additionalContext` with
   a non-empty packet inside the host's hook budget (module deadline 3000 ms).
2. That holds with Hub **off**: bounded one-shot execution, degraded honestly (freshness
   not consulted, Blueprint omitted with a receipt) but never empty because a resident is absent.
3. With Hub **on**, the same hook reuses the warm resident (instant freshness, warm graph)
   and the packet is richer, not merely present.
4. Nothing in the packet is fabricated: the H8 ceiling is a real host observation, and every
   omission (freshness, provider timeout, budget drop) is in the receipt.

Explicit `membrane_context` / CLI federate stays the on-demand enrichment path. It is not the
context system; the hook loop is.

## 2. Where the installed 0.1.24 actually was (verified 2026-09-11)

| # | Blocker | Evidence | State |
|---|---|---|---|
| 1 | Recall module posted only to the resident Hub, using a development token path (`tools/.cache/memory/api-token`) that does not exist on an installed machine. | `hook_diagnostics.rs` original `resident_recall`; installed hook returned `memory_unavailable` in ~1 ms. | Fixed locally (`091a7e61`): installed runtime token/port, resident probe then in-process one-shot route. |
| 2 | Hook sent `remainingContextCeiling: null`; the native route refuses null and requires a complete H8. Claude Code hook input carries no capacity fields. | 400 `h8_invalid` then `h8_unavailable` from `native_route_response`. Push canon AUD-PUSH-07 names this gap. | Fixed locally (`091a7e61`): ceiling derived from the transcript's last main-chain assistant `usage`, provenance `claude_code`, window from `MEMBRANE_HOST_CONTEXT_WINDOW_TOKENS` (default 200k). No transcript → honest `memory_unavailable`. |
| 3 | Hub-less Blueprint status is a cold reconcile: >30 s on this repo, overshoots short deadlines by ~2 s; the freshness pre-step consumed the whole hook budget before any provider ran. | `membrane cli blueprint status --deadline-ms 800` → 2810 ms; `--deadline-ms 30000` → 44.9 s, both `deadline_exceeded`. | Fixed locally (`ac63cf56`): below a 5 s remaining budget the status is not attempted; verdict indeterminate, reason recorded, providers get the budget. |
| 4 | With nothing listening on 47851, a loopback TCP connect takes a full 2 s on this machine instead of an immediate refusal. Every "is the Hub up" probe burns its whole timeout: hook resident probe (600 ms), MCP executor health (2 s), `membrane status` (2.7 s). | `curl -m 5 127.0.0.1:47851/health` → total 2.04 s, HTTP 000; MCP `membrane_context` with 20 s deadline returned in 2005 ms at the auth stage. | Open. |
| 5a | No Codex prompt hook exists at all: activation reconciles Claude hooks only, and the Codex Arcane wrappers resolve to missing files. | `CODEX-AMBIENT-CONTEXT-DIAGNOSIS.md` (2026-09-11), findings 1–2. | Open. |
| 5b | The transcript-derived ceiling is Claude-shaped and assumes a 200k window; recall collapses every failure to `memory_unavailable`. | Same diagnosis, findings 3 & 5. | Open: host-specific budget adapters, typed reasons. |
| 5 | Even after 1–3, the installed hook still hits `module_deadline_exceeded` at ~3.1 s with Hub off. | Installed run after `ac63cf56`. | Open; #4 accounts for 600 ms, the rest is unmeasured provider cost per fresh process (Cortex store open + embedding model load, Ledger owner open, Blueprint provider query). |

## 3. The path

### Step A — make resident detection fail fast (blocker 4)
Do not discover the resident with a TCP connect on a port that may black-hole. Options in
order of preference:
1. A resident liveness marker owned by the controller (pid + port + start time in the
   installed state dir, removed on drain) checked before any socket; connect only when
   the marker is present and its pid is alive.
2. If a marker is unacceptable, cap every resident probe at ≤100 ms and treat timeout as
   absent; on this machine that is still 100 ms wasted per module.
Apply the same rule to `mcp_executor` health, `membrane status`, and `resident_healthy`.
Acceptance: Hub off, `membrane status` < 300 ms; hook resident stage < 100 ms.

### Step B — measure and budget the one-shot providers (blocker 5)
Instrument the one-shot route with per-stage timings in the receipt (store open, bindings,
freshness, each provider, selection). Run the installed hook with Hub off and read the
receipt instead of guessing. Then:
- Cortex: if embedding model load dominates, recall must fall back to lexical/hybrid-without-
  embeddings under a short budget and say so in the receipt; never load a model per prompt.
- Blueprint provider: under a short budget with no warm generation, omit with a receipt
  rather than dispatch a cold query.
- Ledger: explicit owner open must be sub-100 ms or omitted.
Acceptance: Hub off, installed hook, real transcript, `additionalContext` non-empty, total
< 2500 ms, receipt lists every omission.

### Step C — qualify with Hub on
Start the tray/Hub, wait for `/health` 200, rerun the same hook payload. Acceptance: packet
includes Blueprint evidence, freshness verdict from the warm status, total well under 3 s.
Also confirm the hook prefers the resident (transport = resident) and does not run one-shot.

### Step C2 — Codex registration and a declared ambient budget policy
Extend activation to reconcile one owned Codex `UserPromptSubmit` handler bound to the
installed `current/membrane.exe hook` (hooks in `~/.codex/hooks.json`, `codex_hooks = true`),
idempotent and ownership-aware, and retire the stale Arcane projections.

Budget: no host produces a remaining-context observation, and the field does not wait for one.
Hindsight's coding-agents plugin (13 harnesses incl. Claude Code and Codex) recalls under a
configured cap (`recallMaxTokens` 1024, `recallBudget` low/mid/high, 10 s timeout, fail-open).
Define the ambient policy the Push canon requires (push.md:253): hook-mode requests carry a
configured attention cap with provenance `configured_cap`, admitted by the planner as an
advisory bound and recorded in the receipt; strict host-observed H8 remains the contract for
explicit `membrane_context`. Remove the transcript-derived ceiling from `091a7e61`: it
manufactures a window and is Claude-shaped.

### Step C3 — typed recall reasons
Replace the single `memory_unavailable` collapse with typed reasons: credential absent,
resident unreachable, authorization denied, H8 missing/invalid, deadline exceeded (stage),
packet unparseable, valid empty retrieval. Diagnose the reason that occurs before touching
scoring.

### Step D — stop lying at SessionStart
`membrane.cortex-status` reports `cortex_unavailable` whenever no Hub is up. Cortex is
available Hub-off by contract. Report resident state and explicit availability separately.

### Step E — freeze acceptance evidence
Two layers. (1) Installed-binary case (Hub off and Hub on): `membrane hook` with a
UserPromptSubmit payload and a real transcript fixture, asserting valid JSON, non-empty
`additionalContext` under budget, H8 provenance recorded, no stdout pollution.
(2) Live-host case per host (Claude Code, Codex): an ordinary prompt tied to a known
authorized source, no MCP call, host event evidence plus Membrane receipt plus
model-visible source content; repeated after compaction/resume and with the resident
holder absent; no duplicate injections; activation/update/deactivation ownership verified. Land it in the same
place other installed acceptance cases live; promote PUL rows only after it is green.

## 5. Foundation grounding (2026-08-31 receipts)

- **Foundation defect:** `MEM-045`/`MEM-046` were closed as "Unclear, no donor" because the
  inventory catalogued memory atoms but not host wiring. A 2026-09-11 survey of all 32 donor
  checkouts found six with an observed `UserPromptSubmit` → `hookSpecificOutput.additionalContext`
  loop, five of them Codex-first-class via `~/.codex/hooks.json` + MCP in `config.toml`:

  | Donor | Hosts | Hook process does |
  |---|---|---|
  | hindsight (`hindsight-integrations/codex/scripts/recall.py`, coding-agents plugin) | Claude, Codex, +11 | thin client → local daemon/cloud; `recallMaxTokens` 1024, 10 s timeout, fail-open |
  | OpenViking (`examples/codex-memory-plugin/scripts/auto-recall.mjs`) | Claude, Codex, Cursor, Trae | Node client → local server; deadline timer; top-K; no-op is `{}` |
  | mnemon (`cmd/memory/setup.go`, `internal/memory/setup/assets/codex_hooks_test.go`) | Codex, Cursor, Trae, Qoder, Claude | Go binary installs SessionStart/UserPromptSubmit/Stop per host; byte-exact output tests. Prompt hook is advisory reminder text, not retrieval (per `FOUNDATION-AMBIENT-CONTEXT-COMPARISON.md`) |
  | superlocalmemory (`src/superlocalmemory/hooks/auto_recall_hook.py`) | Claude, Codex, Copilot, Antigravity, Hermes | ~20 MB process, never loads the engine; enqueues to daemon recall queue; ack-prompt detection; fail-open |
  | graph-memory-starter (`rag/recall_hook.py`) | Claude | in-process SQLite, whole hits under char caps |
  | caura (`plugin/src/context-engine.ts#shouldRecall`) | OpenClaw | eligibility gate inside the host's context-engine slot |

  Cross-check: `FOUNDATION-AMBIENT-CONTEXT-COMPARISON.md` (2026-09-11, 7 repos) reaches the same
  verdict and repair shape; it omits OpenViking and adds the mnemon distinction above. Its budget
  contract is stricter than Step C2: keep fail-closed without host headroom until a quota mode is
  designed and qualified. Both agree the transcript-derived ceiling must go.

  Tool-time only (no prompt injection): GitNexus, rag-rat, code-review-graph, semantica,
  Understand-Anything. MCP only: serena, octocode, graphiti, and the remainder.

- **The shared design Membrane violates:** the hook is a thin, fail-open client with a
  configured cap; retrieval runs in a resident (daemon/server/queue consumer) or a trivially
  cheap local index. No donor runs its engine cold inside the hook. Membrane's hook runs full
  federate + Blueprint freshness + H8 selection + store/model load inside 3 s with Hub off.
  Consequence for Step B: Hub-off ambient recall must be the cheap path (Cortex lexical/
  hybrid over the installed store, no Blueprint dispatch, no model load) and Hub-on the rich
  path; Step A's fast resident detection decides which.
- `MEM-045` Claude hook/agent capability: `implementation=UNKNOWN`, installed path unavailable, action RECONCILE_EVIDENCE (`docs/provenance/foundation/2026-08-31-implementation-comparison/comparison-pass-a.md:258`). The loop was never qualified on the installed product; this lane is that qualification.
- `MEM-043` propagated absolute deadline and `MEM-008` typed `hub_inactive` with zero spawn: installed latency proof absent / unresolved (`competitive-comparison/membrane.md:18,53`). Blocker 4 above is the concrete failure of MEM-008 on this machine.
- `PUL-022` hybrid RRF is the only Pull `DONOR_BETTER` row and it is the hook recall path: `graph-memory-starter/rag/recall_hook.py` keeps keyword results when embeddings are missing, with fixed caps and chunk dedupe (`competitive-comparison/pull.md:42`). Step B's "never load a model per prompt" is that donor behavior.
- Donor inventory atom "Recall eligibility gate" (`donor-corpus/inventory-pass-a.md`): decide cheaply whether a prompt triggers recall at all. Membrane has no gate; every prompt pays the full path. Add it to Step B.

## 4. What is explicitly not on this path
- Fabricating H8 when the host gives no observation (canon forbids it).
- Starting Hub from the hook, or making the hook depend on residency for correctness.
- Widening the hook module deadline as a substitute for fixing cold costs.
