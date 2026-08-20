# Adapt transcript-mining recommendations

## Boundary

Adapt is a post-session correction miner, not Legion's runtime supervisor. Keep contract-version, blocked-close, false-completion, and unchanged-remote circuit breakers in Legion or Arcane. Adapt may report those patterns retrospectively, but must not become their enforcement path.

## Verified parser gaps

`adapt_sessions.parse_codex_session()` currently collects both `response_item` and `event_msg` projections into `Session.messages` without canonical deduplication. It also appends message text before `_keep_turn()` filters injected `<...>` content, so internal goal/context text can remain available to message-level graders even when correctly excluded from `Session.turns`.

This produced two false signals while reviewing one long Codex session:

- mirrored assistant projections inflated repeated messages;
- injected goal context and tool-carried vocabulary could be attributed to user or assistant behavior by downstream graders.

## Recommended changes

1. Add canonical message provenance: `external_user`, `assistant`, `developer`, `internal_context`, `tool_result`, and `subagent`.
2. Make only `external_user` text eligible to create durable preference authority.
3. Exclude injected goal/context, workspace instructions, developer text, tool results, and subagent traffic from user-correction evidence before constructing `Session.messages`.
4. Deduplicate mirrored Codex projections by stable event identifier when present; otherwise collapse adjacent records with the same normalized role, text digest, and timestamp window.
5. Preserve one canonical message plus source-row references instead of deleting provenance.
6. Keep assistant-quality analysis separate from preference admission. Tool output containing retired terminology must not count as assistant language.
7. Report raw rows, canonical messages, eligible user turns, dropped reasons, and deduplicated count in every mining receipt.
8. Preserve Adapt's existing invariant: assistant narration, summaries, and inferred intent never create authority.

## Regression corpus

Add fixtures proving:

- `<codex_internal_context source="goal">` is unavailable to preference and repeated-ask graders;
- mirrored `response_item` plus `event_msg` yields one canonical assistant message;
- tool output containing legacy terminology is not attributed to assistant prose;
- subagent messages remain quarantined from parent user evidence;
- a genuine external user correction remains eligible and keeps its original timestamp;
- deduplication never merges distinct repeated user corrections.

## Acceptance

- Existing preference-mining results remain stable for clean transcripts.
- Every admitted candidate traces only to canonical external-user rows.
- Reprocessing the 2026-08-11 Codex incident yields no internal-goal re-asks and no mirrored-message inflation.
- Runtime loop detection remains explicitly out of Adapt scope.
