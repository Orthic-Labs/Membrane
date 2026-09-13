# Ambient E2E verification

Timestamp: 2026-09-12 (Windows; fresh run)

Scope: installed Claude Code & Codex host-triggered context, current checkout `e4df4d4e`.

## Identities

- Source HEAD: `e4df4d4e` (`main`, matches `origin/main` at inspection); no source edits made.
- Installed root: `C:\Users\adrds\AppData\Local\Orthic Labs\Membrane\current`.
- Installed Membrane: `0.1.24`; `releaseGeneration=sha256:609c14c37b997b397cce3f81c6f240b236198a626fa5f3d6fb3b14eee0678632`.
- Installed `membrane.exe` SHA-256: `d270593b7f8e262f3c585ee3ad5bea7feea60ebe5978f563101cff05abff5fc7`.
- Claude: `2.1.263`, `C:\Users\adrds\.local\bin\claude.exe`.
- Codex: `0.154.0`, pnpm shim resolves `C:\Users\adrds\AppData\Local\OpenAI\Codex\bin\7ac07f4ce733f89a\codex.exe`.

## Results

| Area | Result | Evidence |
|---|---|---|
| Claude registration | PASS registration / FAIL execution compatibility | `.claude/settings.json` has one Membrane command per configured lifecycle event, including `UserPromptSubmit`; foreign guard hooks remain present. Real Claude stream trace shows Membrane invoked. |
| Codex registration | FAIL | `.codex/config.toml` has `[mcp_servers.membrane]` only; no hook/lifecycle registration. `codex exec` emitted no Membrane hook event. |
| Installed prompt hook | FAIL delivery | Installed `UserPromptSubmit` with isolated temp git repo returned 21–27 ms, `additionalContext:""`, `memory-recall=unavailable:memory_unavailable`. Current workspace query behaved identically. |
| Real Claude ordinary prompt | FAIL host run | `claude -p --output-format stream-json --include-hook-events --verbose --no-session-persistence` invoked installed Membrane. Process exited 1: Claude rejected Membrane `SessionEnd` response because `hookSpecificOutput.hookEventName=SessionEnd` is outside Claude's accepted schema. Trace shows SessionStart `cortex_unavailable` & UserPromptSubmit `memory_unavailable`; no Membrane context injection was observed in captured host events. Raw trace: `C:\Users\adrds\AppData\Local\Temp\membrane-claude-e2e-c7dfb3da8d2f42019806155e406cd9b0\claude-debug.log`. |
| Real Codex ordinary prompt | FAIL ambient registration/injection | `codex exec --json --ephemeral` completed in 8.6 s without calling Membrane MCP; no Membrane lifecycle hook event was emitted. Model response said “Current installed Membrane version isn’t included in automatic context.” This supports the observed absence but is not model-input proof. Raw response: `C:\Users\adrds\AppData\Local\Temp\membrane-codex-e2e-24c453e2f4694243bfb9747b8093fc91\last.txt`; registration snapshot: `codex-config-registration-snapshot.txt`. |
| Hub-on/off | UNTESTED as actual resident comparison | No safe isolated holder fixture was exposed; installed hook fell back to unavailable one-shot with no packet. Current probe did not mutate/stop live services. |
| Six subsystem path | UNTESTED end-to-end / typed degradation observed | No injected packet means Pull publication, Blueprint persisted-generation evidence, Cortex recall, Ledger discovery, Adapt proposal publication, & Push reduction cannot be host-qualified. Hook reports typed `memory_unavailable`; no silence. |
| Receipt/fallback gate | FAIL/UNTESTED | Installed PreToolUse only applies diagnostics fence. With `MEMBRANE_DIAGNOSTICS_ENFORCE=1`, `pnpm test` returned `diagnostics-fence=blocked:fence_not_cleared` in 899 ms; `cat README.md` skips fence in 125 ms. No information-need alternate-retrieval gate is present in this path. |
| Failure handling | PASS typed status / FAIL host compatibility | Native hook exits 0 & records typed unavailable/blocked states. Claude still treats `SessionEnd` event-name output as invalid, causing CLI exit 1. |
| Focused source tests | PASS | `tests/clients/client-matrix.test.mjs`, `tests/e2e/mbr801-evidence.test.mjs`, & `scripts/qualification/cases/mem-windows.test.mjs`: 23 tests passed. |
| MCP suite | UNTESTED | `pnpm run test:mcp` entered managed RightKit (`rightkit: request …`) & did not finish within 60 s; process was interrupted. No direct Cargo was run. |

## Hook measurements

Isolated installed-hook matrix used fresh temp git repos & synthetic host payloads:

- `SessionStart`: 859 ms, `cortex_unavailable`.
- `UserPromptSubmit`: 21 ms, `memory_unavailable`, empty additional context.
- `PreToolUse` `pnpm test`, enforcement enabled: 899 ms, `fence_not_cleared`.
- `PreToolUse` `cat README.md`: 125 ms, `fence_not_applicable`.
- `PostToolUse` without active trace: 104 ms, `observe_no_trace`.
- `Stop`, enforcement enabled: 889 ms, `fence_not_cleared`.
- `SessionEnd`: 43 ms, `session_closed`, but invalid host event name for Claude.

## Precise source findings

- `engine/crates/membrane/src/activation.rs:521` calls `reconcile_claude_hooks`; no `reconcile_codex_hooks` symbol exists. Claude event list is at lines 1698–1699.
- `engine/crates/membrane-runtime/src/hook.rs:125` maps `UserPromptSubmit` recall to `memory_unavailable` when no packet is returned.
- `engine/crates/membrane-runtime/src/hook.rs:141` emits `SessionEnd` as native hook event, which caused the observed Claude schema rejection.
- `engine/crates/membrane-runtime/src/hook_diagnostics.rs:68–131` gates diagnostics via `MEMBRANE_DIAGNOSTICS_ENFORCE` & runs bounded resident/one-shot recall; no information-need fallback authorization is implemented there.

## Ordered next actions

1. Add/fix installed Claude projection so emitted hook event names match each host event's schema; specifically map or omit `SessionEnd` from `hookSpecificOutput` while retaining typed receipt metadata.
2. Give installed Membrane an actual Codex lifecycle owner & registration, then rerun a real `codex exec` prompt with authoritative hook/model-input trace.
3. Make installed runtime expose an isolated resident/one-shot fixture seeded with a unique marker, then prove marker reaches Claude & Codex model input without explicit Membrane tool calls.
4. Add host-level tests for Hub resident on/off, resume/compaction, duplicate suppression, configured cap plus real headroom, & scoped fallback authorization.
5. Rerun managed `pnpm run test:mcp` after RightKit availability is restored; no completion claim until both hosts show prompt-time context delivery.

Status: no blanket completion. Claude registration executes but fails host schema on SessionEnd & has no retrieved context; Codex has callable MCP only & no automatic context path.

## Raw evidence

- Installed UserPromptSubmit request/response: `installed-hook-userprompt-raw.json`.
- Real Claude debug trace & hook rejection: `C:\Users\adrds\AppData\Local\Temp\membrane-claude-e2e-c7dfb3da8d2f42019806155e406cd9b0\claude-debug.log`.
- Real Codex response: `C:\Users\adrds\AppData\Local\Temp\membrane-codex-e2e-24c453e2f4694243bfb9747b8093fc91\last.txt`.
- Focused test output: `e2e-focused-tests-output.txt` (23 passed).

## Untested acceptance scenarios

Actual Hub resident on/off comparison; restart/reconnect; resume; pre/post compaction; duplicate suppression across hook sources; configured-cap versus observed host headroom; automatic versus explicit capacity policy; seeded unique-marker packet reaching each host's authoritative model request; provider failure versus irrelevance versus no-match accounting; Cortex durable preference recall; Blueprint persisted-generation retrieval; Ledger document discovery; Adapt proposal lifecycle; Push protected-span reduction; scoped fallback authorization after matching failure; denial after unrelated/stale/fake/success receipt; alternate retrieval classification for information-reading Bash versus build commands; full managed MCP suite completion.

## Immediate repair cause

First restore provider availability or a supported isolated one-shot seed so `UserPromptSubmit` returns a non-empty packet, then fix Claude projection to omit/map invalid `SessionEnd` hookSpecificOutput. Codex still requires an installed lifecycle registration before any ambient packet can be observed.
