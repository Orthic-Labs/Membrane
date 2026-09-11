# Why Membrane is not established as Codex's ambient context

Date: 2026-09-11. Scope: Codex automatic context delivery, not a full Membrane subsystem audit. Diagnosis & repair plan only; no activation, installation, or product-code changes performed.

Follow-up: [comparison against refreshed foundation repos](FOUNDATION-AMBIENT-CONTEXT-COMPARISON.md) grounds repair choices in seven donor implementations after refreshing all 34 corpus checkouts. It distinguishes host-supplied headroom from configured injection quota & adds concrete host integration references. Installed observations below retain their original snapshot boundary.

## Verdict

Membrane's callable MCP surface is registered, but its automatic Codex delivery path is missing from inspected registrations. Existing Codex Arcane wrappers also resolve to missing files. Separately, an installed Membrane recall-module probe returned no context.

These are distinct integration gaps; the synthetic recall result alone does not establish a retrieval defect. A live daemon, exposed MCP tools, repaired Arcane wrapper, or successful explicit `/federate` request does not establish ambient delivery. Completion requires an ordinary Codex prompt to receive relevant Membrane context before model execution, without an assistant tool call.

## Observed path

```text
Ordinary Codex prompt
  -> enabled Arcane plugin UserPromptSubmit registration
  -> cached arcane-hook.mjs
  -> missing legion/packages/arcane/host/codex-adapter.mjs
  -> no resolved adapter

Codex MCP registration
  -> installed Membrane current/membrane.exe stdio-mcp
  -> nine exposed tools
  -> explicit assistant invocation required

Required ambient path
  -> Codex UserPromptSubmit hook
  -> installed Membrane hook handler
  -> Membrane planner/federation
  -> hookSpecificOutput.additionalContext
  -> Codex adds packet to model context
```

Official Codex documentation supports `UserPromptSubmit` & `hookSpecificOutput.additionalContext`. It loads hooks from Codex configuration layers & enabled, trusted plugin hooks; Claude settings are a separate host configuration. See [Codex hooks](https://learn.chatgpt.com/docs/hooks).

## Findings & fixes

### 1. No direct Membrane ambient registration found for Codex

**Evidence:** `C:/Users/adrds/.codex/config.toml:247` registers Membrane with `stdio-mcp`. User `hooks.json` registers RightKit PreToolUse & Arcane SessionStart/SubagentStart, not Membrane. Enabled Arcane plugin supplies UserPromptSubmit through its own wrapper. Installed Membrane `current/plugin.json` contains no hooks declaration. Membrane activation calls `reconcile_claude_hooks` at `engine/crates/membrane/src/activation.rs:521`; this does not register a Codex prompt hook.

**Impact:** Tool discovery supplies an on-demand interface, not automatic retrieval. No direct Membrane prompt-to-context connection was found in inspected host registrations.

**Fix:** Extend canonical Membrane activation/installer projection to reconcile Codex hooks explicitly. Register one owned UserPromptSubmit handler targeting installer-owned stable `current/membrane.exe hook`. Preserve unrelated handlers; make activate/update/deactivate idempotent & ownership-aware. Do not depend on a development checkout or Claude settings.

Proposed handler shape, to merge into existing Codex hook configuration rather than replace it:

```json
{
  "hooks": {
    "UserPromptSubmit": [{
      "hooks": [{
        "type": "command",
        "command": "\"C:\\Users\\adrds\\AppData\\Local\\Orthic Labs\\Membrane\\current\\membrane.exe\" hook",
        "timeout": 10,
        "additionalContextLimit": 7000
      }]
    }]
  }
}
```

Treat timeout/context limit as initial integration settings; align them with measured handler budget & planner output. Validate full hook output against Codex before shipping this registration. Plugin hooks must also meet Codex's hook-trust requirements; enabled does not prove trusted or executed.

### 2. Existing Codex Arcane hook wrappers resolve to missing targets

**Evidence:** Cached plugin wrapper at `C:/Users/adrds/.codex/plugins/cache/local-brief/arcane/0.1.5+codex.20260815070842/hooks/arcane-hook.mjs:9` targets `legion/packages/arcane/host/codex-adapter.mjs`. Development wrapper at `D:/Claude/tools/codex-brief-plugin/plugins/arcane/hooks/arcane-hook.mjs:9` targets `legion/src/packages/arcane/host/codex-adapter.mjs`. Both files are absent under `D:/Claude`. Calling each exported `findWorkspaceTarget` with this repository's cwd returned `null`. Wrapper execution would emit `ARCANE_PLUGIN_TARGET_UNAVAILABLE` when resolution fails.

Parent `D:/Claude/.codex/hooks.json` additionally contains `/Volumes/D/claude/...` commands, unsuitable for this Windows checkout if that layer loads.

**Impact:** Existing hook plumbing is stale. This does not prove those callbacks fired in this session, nor that fixing their paths would retrieve Membrane context.

**Fix:** Regenerate Legion/Codex hook projection from its canonical installer-owned runtime. Remove only obsolete owned registrations, refresh installed plugin cache through canonical installation, & eliminate development/macOS paths from Windows projection. Validate target existence & resolver success during installation.

Do not simply repoint to relocated source `legion/src/lib/guard/compat/host/codex-adapter.mjs`. It is a development compatibility adapter. Its shared output renderer at `legion/src/lib/host/arcane/host-runtime-output.mjs:24` returns no output on allowed decisions & Arcane decision context on denied events; this is not evidence of a Membrane retrieval bridge. Membrane delivery needs its own explicit host integration.

### 3. Installed recall module currently returns no context

**Reproduction:** Passed a synthetic UserPromptSubmit envelope to installed binary, without executing other hook modules:

```powershell
$payload = @{
  hook_event_name = 'UserPromptSubmit'
  session_id = 'codex-ambient-diagnosis'
  cwd = 'D:/Claude/membrane'
  prompt = 'How does Membrane deliver context automatically to Codex?'
  client = 'codex'
} | ConvertTo-Json -Compress
$payload | & 'C:/Users/adrds/AppData/Local/Orthic Labs/Membrane/current/membrane.exe' hook-module --id membrane.memory-recall
```

Observed response:

```json
{"id":"membrane.memory-recall","status":"ok","output":{"schemaVersion":1,"kind":"membrane.hook.status","state":"unavailable","reason":"memory_unavailable","detail":null}}
```

**Impact:** Registering hooks alone is insufficient. This probe proves empty recall output for this input, not Cortex scoring failure, universal recall failure, or successful host injection. `status: ok` means module execution returned normally; it does not mean context is available. The synthetic payload omitted `remainingContextCeiling` & transcript usage. Source observed during final validation refuses recall without that evidence, so this is not a valid reproduction of a fully supplied host request.

**Source diagnosis:** `engine/crates/membrane-runtime/src/hook.rs:125` collapses recall errors/None/empty output to `memory_unavailable`. Earlier checked-in recall code used a project-relative token path & authenticated `/federate`; this repository's project-relative token file is absent. No `MEMBRANE_API_TOKEN_FILE` override was present. Token contents were not inspected. This is a plausible failure mode, not a proven binary-level cause.

Current source now resolves installed credentials via runtime identity & includes bounded one-shot federation fallback in `hook_diagnostics.rs`. Source changed during inspection through work outside this report; do not equate it with installed behavior.

**Fix:** Retain installed-root credential/endpoint resolution; keep repository scope separate from runtime storage scope. Preserve one planner/federation route across resident & bounded one-shot execution. Build/install through canonical local development flow, then repeat installed-module probe. Emit typed reasons for credential absence, connection failure, authentication failure, timeout, packet parsing, & valid empty retrieval. Diagnose whichever reason actually occurs before changing scoring.

### 4. Prior explanation misidentified both host & retrieval path

Claude settings currently contain ten Membrane hook event registrations. A past `claudeHooksRemoved: 10` receipt is historical, not proof of current deactivation. Registration still does not prove execution.

The hook module's name is `memory-recall`, but its implementation invokes federation. `hook.rs:36` delegates to `resident_recall`; `hook_diagnostics.rs` calls `/federate`. Protocol descriptor labels do not override this runtime call chain. Source projector `engine/crates/membrane-protocol/src/hook.rs:332` aggregates module context into `hookSpecificOutput.additionalContext`, & CLI dispatcher `engine/crates/membrane/src/modes.rs:1164` serializes it to stdout.

**Fix:** Validate installed production path end to end. Stop using Claude activation, MCP discovery, daemon health, or descriptor names as substitutes for Codex context-delivery evidence.

### 5. Current budget-observation fallback is Claude-specific

Final source inspection found another Codex integration requirement in `hook_diagnostics.rs:121`: `transcript_context_ceiling` parses Claude-shaped assistant records with `/message/usage`, `sessionId`, & Claude token fields; it defaults to a Claude-oriented 200,000-token window. Recall accepts a supplied `remainingContextCeiling`, otherwise tries this parser, otherwise returns no context before federation.

**Impact:** A direct Codex hook registration is only a transport fix. No verified Codex-native remaining-context observation adapter was established here. Feeding a Codex transcript into a Claude-shaped parser, or defaulting its capacity, is not valid host-budget integration. Fresh sessions without previous usage also need explicit handling.

**Fix:** Supply a valid, fresh, session/task-bound remaining-context ceiling from Codex host observations through a supported integration. If transcript parsing is necessary, implement & test Codex's actual record shape separately; do not assume Claude fields or capacity. Distinguish missing host-budget capability from retrieval failure. Preserve planner admission rules; do not manufacture a ceiling to force retrieval through. Validate fresh sessions & post-compaction observations as well as established sessions.

## Repair order & acceptance criteria

1. Reproduce installed recall with a valid host-observed remaining-context ceiling & known, already-authorized relevant source. Resolve typed failure until it returns expected context. A no-match query or missing budget observation cannot establish broken scoring.
2. Ship canonical Codex Membrane hook registration, stable installed executable binding, & clean stale owned Arcane projections. Preserve other user hooks.
3. Exercise full installed `membrane hook` with Codex-shaped input. Require valid JSON, correct event name, bounded execution, nonempty expected context, & no diagnostic stdout pollution. A module-only probe does not validate this layer.
4. In a fresh/resumed Codex task with hook definitions loaded & trusted, submit an ordinary prompt tied to a known source. Require host event evidence, Membrane retrieval receipt, & packet delivery before model execution. Do not invoke MCP to trigger it. Demonstrate model-visible source content; answer quality alone is not proof.
5. Repeat after compaction/resume. Verify retained or refreshed context; add supported lifecycle restoration only where required. Repeat with resident holder absent to validate bounded fallback without claiming daemon residency.
6. Verify activation/update/deactivation ownership, no duplicate injections, correct repository isolation, truncation/omission accounting, & observable empty/error outcomes. Untrusted retrieved text must remain evidence, never new authority.

An ambient engine is host-event-driven: it supplies context automatically when needed. A continuously running daemon alone cannot insert text into Codex model input.

## Evidence boundary

- Installed release metadata: Membrane `0.1.24`.
- Installed `membrane.exe` SHA-256 during probe: `D270593B7F8E262F3C585EE3AD5BEA7FEEA60EBE5978F563101CFF05ABFF5FC7`.
- Source HEAD at final inspection: `ac63cf56d5164eb4f007064c26e19172e3382827`.
- Executed: two wrapper-resolution probes, installed recall-module probe, configuration/source inspection, thread guard.
- Not executed: full host hook, live Codex prompt-injection test, reactivation, rebuild/install, scoring repair, full subsystem audit.

Completion target: **ordinary Codex prompt -> installed Membrane retrieval -> host-accepted additionalContext -> relevant evidence in model input, with no assistant tool invocation.**
