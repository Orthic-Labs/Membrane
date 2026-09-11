# Membrane ambient context: comparison with refreshed foundation repos

Date: 2026-09-11. Companion to [installed Codex diagnosis](CODEX-AMBIENT-CONTEXT-DIAGNOSIS.md).

## Answer

We do not need to invent automatic context delivery. Existing references implement host-triggered retrieval, host-specific output, context-engine callbacks, bounded requests, session restoration, & owned hook installation. Membrane has retrieval/planner & hook-output components, but its inspected Codex installation does not connect those into a working automatic path.

The deeper mistake is treating provider availability as host integration. A context service can run perfectly while the host never asks it for context or consumes its response. Our acceptance boundary must be model-visible context on an ordinary prompt.

The earlier report identified installed wiring failures, but omitted these existing implementation references. This comparison supplies that missing grounding. Source observations below prove mechanisms & their callers, not successful deployment of donors on this Windows Codex installation.

## Corpus freshness & scope

Before comparison, fetched upstream default-branch HEAD for every one of 32 shared Membrane donor repos & both local corpus copies. All 34 checkouts matched fetched HEAD afterward; 27 advanced, seven were already current, zero conflicts/errors. These represent 33 distinct repos because Caura has two copies. No local changes existed in those checkouts. Updates were fast-forward only, with no branch creation, reset, or push.

Exact before/after revisions, default branches, & UTC check times are recorded in [shared refresh log](FOUNDATION-CORPUS-REFRESH.jsonl) & [local refresh log](FOUNDATION-CORPUS-LOCAL-REFRESH.jsonl). Freshness means upstream HEAD fetched at those timestamps, not a promise that upstream will remain unchanged.

This is a bounded comparison of automatic host-context delivery, not a new full-canon qualification. Seven relevant repos received focused source inspection: Hindsight, Caura, rag-rat, Mnemon, Mengram, SuperLocalMemory, & Supermemory. Remaining repos were refreshed but not given a full implementation comparison. Canon files & lifecycle states were not changed.

| Repository | Inspected revision | Host surface inspected |
|---|---|---|
| hindsight | `71e74444dd8c3ba9b962f2629878692970a61154` | Native Codex hooks |
| caura-ai__caura | `d68d715d70c79f157987fdfe132376451e0a1243` | OpenClaw context engine |
| cq27-dev__rag-rat | `e0490278868b933e55c0e48552e3f08d455aab43` | Native/VS Code/Cursor hook adapters; local index |
| mnemon | `9c8e760e0143f5d66b5d57609a60e1c4e620da96` | Codex installation & shell hooks |
| mengram | `b2a86fd9de910181a7ebf3ce76bfb72c94b87f4d` | LLM-call middleware |
| qualixar__superlocalmemory | `07a431ed3894ad6b28f144db8a1a8c7d8c839a86` | Codex installation & session context |
| supermemoryai__supermemory | `958ae8b61960fe2ade495b095a01667cf44c5706` | Python agent-framework context provider |

Shared source root: `//192.168.1.7/d/claude/repos/membrane/<repository>/`. Caura was read from its now-current local copy at `D:/Claude/foundation-corpus/membrane-stage3-20260831/caura-ai__caura/`. The accompanying corpus manifest maps evidence paths to these roots.

## Mechanism comparison

These eight rows are scoped engineering questions, not new counted canon atoms. “Best observed” identifies the closest source mechanism for that question; it is not a benchmark or overall product ranking. Confidence is Medium because no donor was installed or exercised through a live Codex prompt here.

| Scope | Domain | Atom | Best observed | Recommended implementation | Why / tradeoffs | Source evidence | Confidence |
|---|---|---|---|---|---|---|---|
| Codex ambient delivery | Host integration | Prompt-triggered retrieval | Hindsight's Codex recall handler | Bind UserPromptSubmit to installed Membrane hook; invoke existing federation automatically & emit returned packet through additionalContext. | Hindsight extracts current prompt, calls recall, then serializes host output; its manifest is a template, not proof of installation. | hindsight: `hindsight-integrations/codex/scripts/recall.py#L87`; hindsight: `hindsight-integrations/codex/hooks/hooks.json#L14` | Medium |
| Codex ambient delivery | Installation | Preserve unrelated host hooks | SuperLocalMemory owned-command reconciliation; Mnemon lifecycle merge | Extend canonical activation with owned Codex entries, stable installed paths, atomic merge, & exact removal on deactivate. | SuperLocalMemory removes only owned commands & preserves mixed groups; Mnemon supplies a simpler lifecycle merge but assumes shell availability. | qualixar__superlocalmemory: `src/superlocalmemory/hooks/codex_hooks.py#L121`; mnemon: `internal/memory/setup/codex.go#L99` | Medium |
| Ambient delivery | Host budget | Bound context without inventing capacity | Caura host tokenBudget plus Hindsight retrieval cap are distinct mechanisms | Separate host-observed remaining capacity from configured injection quota; provide Codex observation adapter or explicitly represent unavailable capacity. | Caura receives tokenBudget from its host; Hindsight requests a capped result. Neither proves a configured cap is actual remaining context. | caura-ai__caura: `plugin/src/context-engine.ts#L816`; hindsight: `hindsight-integrations/codex/scripts/recall.py#L147` | Medium |
| Ambient delivery | Output projection | Use host-consumed context field | rag-rat per-host projection; Caura engine result | Keep one planner but serialize via each host's accepted callback/output contract; test delivered model input. | rag-rat varies native, Cursor, & VS Code envelopes. Caura returns systemPromptAddition while preserving incoming messages. Host APIs are not interchangeable. | cq27-dev__rag-rat: `crates/rag-rat-cli/src/agent_hook/mod.rs#L811`; caura-ai__caura: `plugin/src/context-engine.ts#L1020` | Medium |
| Hub-independent delivery | Lifecycle | Read available evidence without watcher | rag-rat SessionStart orientation | Use bounded installed one-shot retrieval when no resident holder exists; report freshness & missing providers. | rag-rat reads an existing index without requiring watcher startup. Its database access is internal to its product; Membrane must retain Blueprint's storage boundary. | cq27-dev__rag-rat: `crates/rag-rat-cli/src/agent_hook/mod.rs#L628` | Medium |
| Codex ambient delivery | Continuity | Inject initial session context | SuperLocalMemory Codex session handler | Use cheap SessionStart orientation & prompt-specific refresh; explicitly test resume & post-compaction restoration. | SuperLocalMemory calls session_init then session-context & emits additionalContext. Its prompt handler only forwards rehash/topic signals; it is not full prompt recall. | qualixar__superlocalmemory: `src/superlocalmemory/hooks/hook_handlers.py#L281` | Medium |
| Ambient delivery | Feedback | Avoid retaining recalled text as new memory | Hindsight content filtering & stable session document ID | If authorized capture is enabled, filter injected spans & submit idempotent candidates to existing Cortex admission after response. | This prevents a recall-to-retention feedback loop. Do not equate donor auto-store with authorization to write Membrane truth. | hindsight: `hindsight-integrations/codex/scripts/lib/content.py#L37`; hindsight: `hindsight-integrations/codex/scripts/retain.py#L125` | Medium |
| Embedded hosts | Context provider | Put retrieval around actual model invocation | Mengram middleware; Supermemory before_run | Where we own host integration, register a before-model context provider calling Membrane once; keep after-response capture separate. | Both insert recalled text before model execution. Their framework callbacks are not native Codex hooks; message-count limits do not prove token safety. | mengram: `mengram_middleware.py#L61`; supermemoryai__supermemory: `packages/agent-framework-python/src/supermemory_agent_framework/context_provider.py#L101` | Medium |

## What each reference actually establishes

**Hindsight:** Codex hook template calls SessionStart readiness, UserPromptSubmit recall, & Stop retention. `recall.py:143-153` requests bounded results; `:189-197` emits `hookSpecificOutput.additionalContext`. This is a concrete automatic retrieval-to-output mechanism, unlike instructions telling the model to recall. Its `__SCRIPTS_DIR__` placeholders need installation-time resolution; this review did not prove that deployment. Recall refuses daemon startup (`:106`), so missing daemon still prevents injection. Empty/error paths frequently produce no context; some errors go to stderr. Keep observable Membrane degradation rather than copying that silence.

**Caura:** `plugin/src/index.ts:768` registers a context engine. `config.ts:108` checks the host's selected context-engine slot; registering tools alone is insufficient. `context-engine.ts:816,936-942,1012-1046` consumes host tokenBudget, allocates recall space, & returns `systemPromptAddition` plus original messages. It estimates tokens as characters/4 (`:465-478`), so it is not evidence of exact token accounting. Reuse host ownership/callback structure, not its OpenClaw API or guessed capacity for Codex. Its compaction method delegates to host runtime; this does not establish Codex compaction support.

**rag-rat:** `agent_hook/mod.rs:606-622` dispatches native events into retrieval/output paths. SessionStart reads an existing local index, refuses an absent index, & reports newer schema instead of pretending compatibility (`:628-670`). Output formatting varies by host (`:811-835`). This demonstrates a watcher-independent read path. Its post-edit handler may spawn detached reindexing when watcher is absent (`:855-878`); do not import that behavior where Membrane's background-holder policy forbids it. Many errors are swallowed (`:601-603`); Membrane should retain typed diagnostics.

**Mnemon:** `internal/memory/setup/codex.go:99-164` installs lifecycle hooks while preserving unrelated entries. However, `internal/memory/setup/assets/codex/user_prompt.sh` only asks whether recall is needed. That is advisory prompting, not automatic semantic retrieval. This is precisely the category error we must avoid: counting a hook or reminder as delivered context. Session start inserts status/static guide text; it is not equivalent to a task-specific fused packet.

**SuperLocalMemory:** `hook_handlers.py:281-308` implements actual Codex SessionStart context insertion; `codex_hooks.py:121-143` supplies owned atomic configuration merge. Prompt handler `:311-323` runs rehash/topic actions rather than its own retrieval. Its Claude cache injector is a different path; do not claim Codex inherits it. This is useful installer/session evidence, not proof of complete per-prompt Codex recall.

**Mengram & Supermemory:** Their middleware/context-provider lifecycle wraps model execution itself. Mengram recalls before request & stores selected messages afterward; Supermemory extends instructions/messages in `before_run` & optionally stores conversation in `after_run`. Neither inspected surface provides native Codex installation. These are useful architectural references for hosts we control, not drop-in replacements for Codex.

## What we are doing wrong

1. **Closing the wrong acceptance boundary.** We have been checking tools, packets, & service liveness. Donor source makes the missing boundary visible: a host callback must call retrieval & pass its result into model input.
2. **Treating host support as interchangeable.** Claude hooks, Codex hooks, OpenClaw context-engine slots, & framework middleware have distinct registration, payload, & output contracts. Membrane needs a real Codex projection, not merely an MCP entry or Claude-shaped fallback.
3. **Requiring an observation without supplying its producer.** Current Membrane source requires a remaining-context ceiling or Claude transcript-derived substitute. Hindsight's configured retrieval cap & Caura's host-supplied tokenBudget show two different concepts. Our adapter must not relabel a quota as measured remaining capacity. If Codex cannot expose the required observation, that is a named product-policy decision, not a scoring defect or something to hide behind memory_unavailable.
4. **Conflating background residency with foreground retrieval.** Hub should improve residency/background work. A valid authorized bounded foreground request should use existing one-shot path when Hub is absent. rag-rat demonstrates local reads without a live watcher; Hindsight demonstrates why coupling recall to daemon startup can still leave prompts empty.
5. **Failing to distinguish reminders from delivery.** Mnemon's prompt hook is real but advisory. Hindsight's prompt hook actually retrieves. Our tests must distinguish these behaviors.
6. **Missing installed-host qualification.** Source wiring, installer registration, callback execution, packet construction, & model-visible insertion are separate states. The report must not merge them into “working.”

## Concrete repair design

This is a proposed adaptation of observed mechanisms, not a claim that repair is implemented.

**Primary path:** canonical installer registers one owned Codex prompt handler targeting installed Membrane. Host supplies prompt, repository/worktree, session/task identity, & supported budget observations. Handler calls existing Membrane planner/federation; host adapter serializes accepted packet as additionalContext. No explicit assistant call is needed.

**Readiness & fallback:** validate installed generation, authorization, repository binding, & budget capability. Prefer resident endpoint when ready; otherwise invoke bounded installed one-shot route with remaining time. Provider absence, stale evidence, empty search, admission rejection, & missing host-budget capability receive distinct receipts. Do not start persistent background work merely to satisfy a hook.

**Budget contract:** keep configured injection quota separate from measured host headroom. With valid headroom, bound publication by both. Without headroom, current fail-closed contract remains in force until an explicitly designed host allocation/quota mode is implemented & qualified. A hard small quota is a possible supported-mode design, not permission to forge current required ceiling evidence. Fresh sessions & compaction must be included in this decision.

**Continuity:** add bounded initial orientation & rehydration through supported lifecycle callbacks. Track successful injections by host/session/turn/event to prevent duplicates across multiple hook sources. Optional after-response capture must strip injected text & go through Cortex admission; this report does not authorize automatic durable writes.

**Delivery proof:** installation twice preserves foreign hooks & yields one Membrane handler; real Codex prompt invokes it; expected source is in hook output & model input before response; repeat with Hub absent, after resume/compaction, & with invalid/stale authorization/budget evidence. Assert typed omissions & no duplicate context. Repeat installed-source identity checks after updates.

No new context engine, scoring rewrite, or second planner is justified by these findings. First repair host connection, budget producer/contract, & installed-path qualification using existing Membrane components.

## Reuse boundaries & completion

Observed root licenses: Hindsight, rag-rat, Supermemory — MIT; Caura, Mnemon, Mengram — Apache-2.0; SuperLocalMemory — AGPL-3.0 text. No donor code was copied. Recommendations are mechanism references; source reuse needs file-level license/notice review, especially before taking SuperLocalMemory code. No license compatibility determination is made here.

Requested: refresh corpus first, then ground ambient-context repair in studied repos. Evaluated: all 34 checkout revisions plus seven donors across eight scoped questions. Unresolved: live Codex budget capability, installed end-to-end injection, & Hub-on/off runtime qualification. Excluded: full 33-repo/all-atom requalification, donor installation, source-code implementation, durable-memory writes. These exclusions do not change the required repair acceptance boundary.

**Result: existing references cover the integration patterns. Our immediate work is connecting & qualifying Membrane's host-facing path, not reinventing retrieval.**
