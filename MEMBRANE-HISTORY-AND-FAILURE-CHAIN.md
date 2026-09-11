# Membrane: origin & failure chain

Historical investigation, 2026-09-11. Scope: tracked history in `bogusyogi/claude` & `Orthic-Labs/Membrane`, from original context tooling through current native runtime. No runtime changes performed.

## Finding

**Membrane started as a system for getting useful context into an agent automatically. That intent was present before its name, before Hub, & before its standalone repository.** It subsequently acquired automatic recall hooks for Claude & Codex, federation, host adapters, continuity, & delivery accounting.

It did not fail at one clean turning point. History shows repeated migrations breaking deployment or dropping host wiring, while acceptance increasingly certified intermediate artifacts: a packet, a file write, an MCP registration, or a running service. Those are prerequisites; none alone establishes that relevant context entered a particular host's current model request.

**My earlier claim that automatic Codex context never became an explicit requirement was too broad & wrong.** Earlier documentation & hooks establish that intention. Later canon narrowed Codex's recorded capability to L2 MCP/tool receipts/response gates, with pending qualification. That failed to preserve & enforce earlier ambient behavior across replacements.

## Evidence method

Both repositories have non-shallow history. Investigation used historical trees, diffs, commit messages, symbol searches, & deletion/replacement comparisons. Some historical branches contain equivalent rewritten commits; entries below identify representative commits, not separate incidents for each hash.

Parent snapshot: `13d5dc8f7b8d230f5133fe43a1dae5e75f9ad487`. Product snapshot: `ac63cf56d5164eb4f007064c26e19172e3382827`. Existing uncommitted product work was excluded from historical conclusions.

Code proves what a snapshot implements. Commit messages supply contemporary incident accounts, explicitly identified below. Neither establishes every historical machine's installed state. Dates below are commit dates; this is a source-level failure chain, not invented dates when every user's context stopped working.

## 1. June 27–July 2: context economy, then durable memory

Earliest relevant predecessor located is parent [4033c07a](https://github.com/bogusyogi/claude/commit/4033c07a), June 27: native token-compression tooling alongside Blueprint changes. Original substrate was context reduction, not a standalone daemon.

Parent [9656fe00](https://github.com/bogusyogi/claude/commit/9656fe00), June 30, adds `tools/lib/memory/mem.py` & `tools/lib/CONTEXT-ENGINEERING.md`. Documentation deliberately renames COMPACTION to Context Engineering because compression alone hid retrieval & curation. It defines three families:

- Push: reduce material entering context.
- Pull: rank & inject durable knowledge.
- Persist: curate knowledge across sessions.

Its end-to-end diagram explicitly routes durable memory/Blueprint into “my context / agent context.” Recall trigger is “session start + on-demand.” This is direct evidence of an ambient objective, although it does not imply every layer was already automatically intercepted.

June 30 [1394355c](https://github.com/bogusyogi/claude/commit/1394355c) moves memory execution into Rust MemRight. July 2 [fd86c86f](https://github.com/bogusyogi/claude/commit/fd86c86f) wires shims, policy hooks, & meters; [fb97c28b](https://github.com/bogusyogi/claude/commit/fb97c28b) makes memory hooks repository-carried & installer-registered. July 4 [4811e8f5](https://github.com/bogusyogi/claude/commit/4811e8f5) adds post-compaction recall rearming.

**Original product direction:** retain knowledge, recall it at lifecycle boundaries, reduce incoming material, & preserve continuity. Current six-subsystem vocabulary came later; it should not be retroactively imposed on these first implementations.

## 2. July 7–12: Codex hooks & federation already existed

Parent [f9fbe6dc](https://github.com/bogusyogi/claude/commit/f9fbe6dc), July 7, explicitly wires Codex to shared memory engine. In `tools/codex-brief-plugin/plugins/brief/hooks/hooks.json`:

- SessionStart invokes `hooks/memright-start.cjs`.
- UserPromptSubmit invokes `tools/hooks/recall_memory.py`.
- Ingestion is also registered.

`memright-start.cjs` starts memory service when unhealthy & returns `hookSpecificOutput.additionalContext`. Its startup text announces shared per-prompt recall; actual recall belongs to the separate prompt hook. This is source wiring, not proof every Codex distribution loaded that plugin successfully.

Parent [bf1fd921](https://github.com/bogusyogi/claude/commit/bf1fd921), July 12, introduces federation routing across Python & Codex recall planners, gateway/providers, Rust CLI, & MCP client. Thus the later distinction “ambient equals only Cortex; federation is necessarily explicit” is historically false. Automatic paths have called federation.

But this snapshot already contains an important gap. Claude planner defaults to `shadow`: federation runs for telemetry while legacy recall supplies context. Its `on` branch uses `_format_packet_block()`, which deliberately emits a content-free summary—trace, provider status, budget, block counts, & receipt IDs—and excludes candidate text. **Automatic federation invocation already existed; automatic delivery of its retrieved knowledge was not established by that implementation.** This is an earlier evidence/content mismatch than August's disk-write check, independent of July 26's deployment outage.

## 3. July 26: consolidation breaks deployment & federation

RightContext/MemRight-related source consolidates under Membrane & moves into its own repository. Product [e7158989](https://github.com/Orthic-Labs/Membrane/commit/e7158989) & parent [3ecf44e1](https://github.com/bogusyogi/claude/commit/3ecf44e1) mark consolidation; parent [f693c3e6](https://github.com/bogusyogi/claude/commit/f693c3e6) links standalone repository.

Parent repair [b31967cc](https://github.com/bogusyogi/claude/commit/b31967ccd5b5235e852a84e80d4288bc3c27d2ed) records a concrete outage:

1. Renamed setup flags retained eight old attribute references, crashing setup.
2. Installed hooks remained July 23 copies while source had advanced.
3. Existing binaries searched deleted `tools/memright` gateway path. Federation failed & prompt handling fell back to legacy recall.
4. Manifest path changed without matching source commit/digest, invalidating installation verification.

Diff repairs those references, resolves both gateway layouts, passes explicit federation-script path, & restores a verifiable manifest.

**First concrete deployment/federation break established here:** extraction changed ownership & paths without preserving installed consumers. This was repaired; it is not proof of uninterrupted failure from July onward. It establishes a recurring failure pattern early.

## 4. August 3: delivery proof stops at a file write

Parent [265f3589](https://github.com/bogusyogi/claude/commit/265f35892c1940595fa6b056dbcd71aa324ba42c) claims a real Codex L2 adapter. It writes a fused packet into an AGENTS.md marker block, reads that file back, & can report `context_enforced` plus `packet_delivered` with `origin: host`.

Evidence: `tools/codex-brief-plugin/plugins/membrane/lib/agents-context.cjs`, especially `writeAgentsFile()` & `injectContext()`.

**That check proves bytes landed on disk. It does not prove an already-running Codex turn reread them or included them in its model request.** This is a concrete mismatch between evidence & claimed delivery, even when packet generation is perfect.

Commit message also says Codex had no lifecycle hook. July 7 source disproves that as a universal historical statement; at most it describes a particular active installation or missing fused-packet path. The history already contained a distinction that later explanations flattened.

## 5. August 5–21: real adapters, repeated moves, unreproducible projection

Product [03d638a2](https://github.com/Orthic-Labs/Membrane/commit/03d638a24ba7f668234dcbabaaceb5c9842b93db), August 5, consolidates renderers & session accounting. Its message acknowledges separate renderers drifting while tests passed one path.

Product [57bb7943](https://github.com/Orthic-Labs/Membrane/commit/57bb794389b35ea8262cb342cd6c088ea926813d), August 9, moves host adapter into `mcp/host/context-adapter.cjs`. This adapter calls resident `/federate`, falls back to client execution, renders a packet, & emits `hookSpecificOutput.additionalContext`. This is an actual context-output implementation, not merely an MCP descriptor.

Parent [5f0a533f](https://github.com/bogusyogi/claude/commit/5f0a533f386e71430678f2f329980729e26a4eb4), August 12, replaces older hook scripts with Membrane entrypoint. Parent [45f37dc3](https://github.com/bogusyogi/claude/commit/45f37dc35a88a09332698ab28f1a273628b3d683), August 20, replaces Crypt plugin with Cortex. **These deletions had replacement registrations; deletion alone is not evidence of lost functionality.**

Product [a7ba0235](https://github.com/Orthic-Labs/Membrane/commit/a7ba02350383a14703a86d39b043c7c6b7b6e3f3) explicitly describes Codex SessionStart, UserPromptSubmit, PreCompact, & PostCompact as native seams in host capability matrix.

Parent [e963c3ce](https://github.com/bogusyogi/claude/commit/e963c3ce), August 21, consolidates Codex projection onto `bin/membrane-host.cjs`. Every relevant hook invokes it. However:

- That executable source is absent from committed tree.
- Projection test asserts it exists locally.
- Repository-wide search for its name finds manifest, README, & tests, but no tracked generator.
- Root `.gitignore` ignores `bin/`, with Rust `src/bin` exceptions only.

**Proven defect:** committed projection is not self-contained for a fresh checkout. Ignored local source is a plausible explanation, not established intent. A local passing test could conceal this packaging failure. Historical installed copies may have contained extra files.

## 6. August 28–September 5: installed boundary retains MCP, loses tracked Codex projection

Product [20b4b6c5](https://github.com/Orthic-Labs/Membrane/commit/20b4b6c5), August 28, binds installed production runtime. `activation.rs` reconciles Codex & Claude **MCP** registrations, but automatic hook reconciliation is specifically `reconcile_claude_hooks()`.

That leaves Codex automatic context dependent on a separate projection. Parent [a1193145](https://github.com/bogusyogi/claude/commit/a1193145), September 5, deletes that tracked Membrane plugin, its hook manifest, marketplace, & host-projection test. It also deletes older workspace host bridge/tests. Setup source still contains references/comments describing those now-removed projections.

**This is the clearest source-level loss of Codex delivery ownership:** old projection is removed while native product activation supplies Codex MCP rather than equivalent Codex lifecycle installation. A daemon can be alive & tools registered with this edge entirely missing.

## 7. August 30 canon: narrower capability, pending qualification

Product [c6cfbca9](https://github.com/Orthic-Labs/Membrane/commit/c6cfbca9), August 30, introduces normalized ledgers. Historical `docs/current/atoms/membrane.md` records MEM-046 as:

> Project honest Codex CLI + cdx MCP/tool-receipt/response-gate capability up to declared L2.

Qualification MEM-Q046 asks for generic reconciliation against released consumer & remains PENDING. Current `docs/canon/membrane.md` retains that narrow capability & pending qualification.

**Canon did not erase all context ambition. It failed to make automatic Codex delivery a non-negotiable acceptance condition for replacement.** Earlier hooks & native-seam matrix were not carried into a concrete released-host test: submit a prompt without a Membrane tool call & observe relevant fused context entering that same turn.

This canon problem follows earlier evidence-boundary mistakes; it cannot explain away their origin or be blamed as sole cause.

## 8. September 9–11: native recall also ships with missing prerequisites

Product [eab1e601](https://github.com/Orthic-Labs/Membrane/commit/eab1e601), September 9, adds native `hook.rs` & `hook_diagnostics.rs`. Production `NoNativeHookService` overrides recall to call `resident_recall()`; its name & trait's default `Ok(None)` do **not** mean production recall was an empty stub.

Initial `resident_recall()` posts only to resident Hub, resolves token through environment/development cache, & forwards nullable remaining-context ceiling without producing host capacity evidence. Product [8af768ac](https://github.com/Orthic-Labs/Membrane/commit/8af768ac), September 10, advances installed native caller cutover.

Product repair [091a7e61](https://github.com/Orthic-Labs/Membrane/commit/091a7e6183a3acfdc6d5cd280f586a7f7d172e84), September 11, explicitly records that missing capacity evidence & rejected null ceiling prevented Claude ambient delivery. Diff adds installed endpoint resolution, one-shot native federation fallback, task binding, & capacity observation from Claude transcript usage. Later same-day commits adjust timeout & freshness behavior.

**Separate defect:** even a registered hook could reach federation without satisfying its admission prerequisites, yielding no context. This is not established as a Cortex scoring/scope failure. Claude-specific repair also does not install a Codex adapter or create Codex capacity evidence.

## Where it actually went wrong

| Failure | Concrete evidence | Required correction |
|---|---|---|
| Context output could contain accounting instead of knowledge | July 12 federation formatter excludes candidate text | Verify admitted evidence content reaches host, separately from metadata & receipts. |
| Migration outpaced installed-consumer preservation | July 26 setup/gateway/manifest incident | Test upgrade & clean installation against real host bindings before retiring paths. |
| Producer success was promoted to delivery success | August 3 AGENTS.md readback marked host delivery | Separate generated, materialized, host-included, & observed-consumed states. Never infer later state from earlier one. |
| Local files hid incomplete shipping source | August 21 missing `bin/membrane-host.cjs` | Package from tracked source in a clean checkout; verify every installed hook target. |
| Codex ambient ownership fell between repositories | Native activation is Claude-hook-specific; September 5 removes Codex projection | Give installed Membrane an explicit Codex lifecycle owner & supported host injection mechanism. |
| Runtime caller omitted planner prerequisites | Native resident-only recall lacked authentic host capacity | Produce host-specific, task-bound capacity evidence; preserve bounded execution without Hub. |
| Canon described weaker surface than intended experience | MEM-046 L2 scope & generic pending qualification | Restore prompt-triggered delivery, continuity, & host-observed acceptance as explicit capabilities. |

## Repair target derived from history

1. Preserve one product contract: relevant context reaches a supported host's turn automatically, without asking the model to call Membrane. Explicit tools remain separately available.
2. Bind each host to installed runtime through an actually supported lifecycle/injection surface. For Codex, validate host version & real event consumption; neither a manifest nor an AGENTS.md write is sufficient proof.
3. Route automatic requests through planner with authentic host budget, task identity, source authorization, & omission reporting. Exercise relevant Pull/Cortex/Blueprint/Ledger inputs; Adapt remains proposals & Push faithful reduction under planner policy.
4. Test clean installation, upgrade, restart, compaction, Hub on, & Hub off. Hub controls background residency; it cannot be used as a substitute for proving delivery. Prompt-time bounded work must follow explicit lifecycle policy.
5. Use known, uniquely identifiable context & observe its inclusion in host request or authoritative host trace. Model response can provide supporting evidence. Measure relevance, omissions, & budget, not merely existence of text.
6. Retire previous adapter only after replacement passes that same host-level acceptance. Record unsupported seams honestly instead of silently substituting callable MCP.

History identifies a recurring engineering failure: **we repeatedly preserved the engine while losing or weakening proof of the last edge into the model's context.** Original goal was understandable & recorded. Delivery ownership, migration equivalence, & acceptance failed to protect it.
