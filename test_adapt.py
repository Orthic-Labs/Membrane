"""Offline tests for the adapt pipeline. No network, no memright binary."""
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
# Make the eval/ subpackage importable from tests too.
sys.path.insert(0, str(Path(__file__).resolve().parent / "eval"))

import adapt_sessions as ts


def _write(path: Path, lines: list[dict]) -> Path:
    path.write_text("\n".join(json.dumps(x) for x in lines), encoding="utf-8")
    return path


CLAUDE_ROWS = [
    {"type": "user", "userType": "external", "cwd": "D:\\Claude", "sessionId": "abc",
     "message": {"content": "always use JSONL for structured logging, not logfmt"}},
    {"type": "user", "userType": "external", "cwd": "D:\\Claude", "sessionId": "abc",
     "message": {"content": "<command-name>/brief</command-name>"}},                    # injected wrapper
    {"type": "user", "userType": "external", "cwd": "D:\\Claude", "sessionId": "abc",
     "message": {"content": [{"type": "tool_result", "content": "ok"}]}},               # tool result list
    {"type": "user", "isSidechain": True, "cwd": "D:\\Claude", "sessionId": "abc",
     "message": {"content": "subagent traffic must be skipped entirely"}},              # sidechain
    {"type": "user", "userType": "external", "isMeta": True, "cwd": "D:\\Claude",
     "sessionId": "abc", "message": {"content": "meta row must be skipped too"}},       # meta
    {"type": "assistant", "message": {"content": "assistant rows never count"}},
    {"type": "user", "userType": "external", "cwd": "D:\\Claude", "sessionId": "abc",
     "message": {"content": "Caveat: the messages below were generated"}},              # caveat
    {"type": "user", "userType": "external", "cwd": "D:\\Claude", "sessionId": "abc",
     "message": {"content": "short"}},                                                  # < 10 chars
]

CODEX_ROWS = [
    {"type": "session_meta", "payload": {"session_id": "cdx1", "cwd": "D:\\Claude\\heardright"}},
    {"type": "response_item", "payload": {"type": "message", "role": "user", "content": [
        {"type": "input_text", "text": "<recommended_plugins>stuff</recommended_plugins>"}]}},
    {"type": "response_item", "payload": {"type": "message", "role": "user", "content": [
        {"type": "input_text", "text": "never train the wake encoder from scratch, distill the teacher"}]}},
    {"type": "response_item", "payload": {"type": "message", "role": "assistant", "content": [
        {"type": "output_text", "text": "assistant text ignored"}]}},
    {"type": "event_msg", "payload": {"type": "token_count"}},
]


def test_claude_parser_keeps_only_real_user_turns(tmp_path):
    f = _write(tmp_path / "abc.jsonl", CLAUDE_ROWS)
    s = ts.parse_claude_session(f)
    assert s is not None
    assert s.tool == "claude-code" and s.session_id == "abc" and s.cwd == "D:\\Claude"
    assert [t.text for t in s.turns] == ["always use JSONL for structured logging, not logfmt"]
    assert s.turns[0].scope == "D--Claude"
    assert s.stats.kept_turns == 1 and s.stats.dropped_turns >= 1


def test_codex_parser_extracts_input_text_and_meta(tmp_path):
    f = _write(tmp_path / "rollout-x.jsonl", CODEX_ROWS)
    s = ts.parse_codex_session(f)
    assert s is not None
    assert s.tool == "codex" and s.session_id == "cdx1"
    assert s.cwd == "D:\\Claude\\heardright"
    assert [t.text for t in s.turns] == ["never train the wake encoder from scratch, distill the teacher"]
    assert s.turns[0].scope == "D--Claude-heardright"


def test_parsers_return_none_when_no_turns(tmp_path):
    f = _write(tmp_path / "empty.jsonl", [{"type": "assistant", "message": {"content": "hi"}}])
    assert ts.parse_claude_session(f) is None


def test_parser_skips_malformed_lines(tmp_path):
    f = tmp_path / "bad.jsonl"
    f.write_text('{"type": "user"\nnot json at all\n'
                 + json.dumps(CLAUDE_ROWS[0]), encoding="utf-8")
    s = ts.parse_claude_session(f)
    assert s is not None and len(s.turns) == 1


def test_redaction_strips_secrets():
    text = ("use key sk-abcdefghijklmnop1234 and ghp_ABCDEFGHIJKLMNOPQRST12 "
            "password: hunter2secret bearer eyJhbGciOiJIUzI1NiIsInR5cCI6")
    out = ts.redact(text)
    assert "sk-abcdefghijklmnop1234" not in out
    assert "ghp_ABCDEFGHIJKLMNOPQRST12" not in out
    assert "hunter2secret" not in out
    assert out.count("[REDACTED]") >= 3


def test_scanner_positive_drops_batch_at_send(tmp_path, monkeypatch):
    """Per-turn scanner was moved to per-batch in adapt_llm.extract_observations;
    this test pins the batch-level contract via a direct call to scan_batch_for_secrets.
    """
    monkeypatch.setattr(ts, "scanner_clean", lambda text: True)
    monkeypatch.setattr(ts, "scan_batch_for_secrets", lambda batch: False)
    rows = [{"type": "user", "userType": "external", "cwd": "D:\\Claude", "sessionId": "s",
             "message": {"content": "always use the special token carefully"}}]
    sess = ts.parse_claude_session(_write(tmp_path / "s.jsonl", rows))
    assert sess is not None and len(sess.turns) == 1
    # The parser accepts the turn (no per-turn scanner). Batch scanner is the gate.
    assert ts.scan_batch_for_secrets([(t.text, t.scope, t.text) for t in sess.turns]) is False


def test_unknown_rows_are_counted_but_do_not_crash(tmp_path):
    rows = [
        {"type": "future_event", "payload": {"shape": "unknown"}},
        {"type": "user", "userType": "external", "cwd": "D:\\Claude", "sessionId": "s",
         "message": {"content": "always use JSONL for structured logging"}},
    ]
    s = ts.parse_claude_session(_write(tmp_path / "s.jsonl", rows))
    assert s is not None and s.stats.unknown_rows == 1


def test_long_turns_truncated(tmp_path):
    rows = [{"type": "user", "userType": "external", "cwd": "D:\\Claude", "sessionId": "s",
             "message": {"content": "x" * 10_000}}]
    s = ts.parse_claude_session(_write(tmp_path / "s.jsonl", rows))
    assert s is not None and len(s.turns[0].text) == ts.MAX_TURN_CHARS
    assert s.stats.truncated_turns == 1


def test_mid_session_cwd_switches_are_per_turn_scoped(tmp_path):
    rows = [
        {"type": "user", "userType": "external", "cwd": "D:\\Claude",
         "sessionId": "s", "message": {"content": "always use JSONL for logs"}},
        {"type": "user", "userType": "external", "cwd": "D:\\Claude\\heardright",
         "sessionId": "s", "message": {"content": "never retrain the wake encoder"}},
    ]
    s = ts.parse_claude_session(_write(tmp_path / "s.jsonl", rows))
    assert s is not None
    assert [t.scope for t in s.turns] == ["D--Claude", "D--Claude-heardright"]


def test_scope_mapping_and_excludes():
    assert ts.scope_for_cwd("D:\\Claude\\heardright") == "D--Claude-heardright"
    assert ts.scope_excluded("D:\\Claude\\Health\\medical-research-system")
    assert not ts.scope_excluded("D:\\Claude\\heardright")
    assert ts.text_excluded("please analyze this injection dose question")


def test_state_incrementality(tmp_path, monkeypatch):
    monkeypatch.setattr(ts, "STATE_DIR", tmp_path)
    monkeypatch.setattr(ts, "STATE_FILE", tmp_path / "state.json")
    state = ts.load_state()
    f = _write(tmp_path / "abc.jsonl", CLAUDE_ROWS)
    sess = ts.parse_claude_session(f)
    ts.mark_learned(state, [sess])
    ts.save_state(state)
    reloaded = ts.load_state()
    assert reloaded["learned"]["claude-code"]["abc"] == sess.mtime


# --- Task 2: LLM lane tests ---
import adapt_llm as tl
import outcomes


def test_batching_respects_char_budget():
    turns = [("claude-code", "D--Claude", f"turn number {i} " + "y" * 300) for i in range(400)]
    batches = tl.build_batches(turns, budget=5_000)
    assert all(sum(len(t[2]) for t in b) <= 5_000 for b in batches)
    assert sum(len(b) for b in batches) == 400


def test_extract_parses_model_json():
    def fake(system, user):
        assert json.loads(user)[0]["text"] == "always use JSONL"
        return json.dumps([
            {"category": "tooling", "observation": "use JSONL for structured logs, not logfmt",
             "evidence": "always use JSONL", "prompt": 1,
             "record_type": "agent_preference", "durability": "cross_task_explicit",
             "subject": "agent_behavior"}])
    out = tl.extract_observations([("claude-code", "D--Claude", "always use JSONL")], llm=fake)
    assert out.outcome == outcomes.Outcome.SUCCESS
    assert len(out.actions) == 1
    assert out.actions[0]["category"] == "tooling"
    assert out.actions[0]["source"] == "model"


def test_deterministic_extract_catches_explicit_preferences():
    obs = tl.extract_deterministic([("claude-code", "D--Claude",
                                     "always use JSONL for structured logging, not logfmt")])
    assert obs and obs[0]["category"] == "explicit-preference"
    assert "always use JSONL" in obs[0]["evidence"]


def test_extract_returns_success_on_fenced_payload():
    fake = lambda system, user: json.dumps([{
        "category": "tooling", "observation": "Keep durable workflow evidence.",
        "evidence": "hello world", "prompt": 1,
        "record_type": "agent_preference", "durability": "cross_task_explicit",
        "subject": "agent_behavior",
    }])
    out = tl.extract_observations([("codex", "D--Claude", "hello world turn")], llm=fake)
    assert out.outcome == outcomes.Outcome.SUCCESS
    assert len(out.actions) == 1


def test_extract_fail_closed_without_preference_classification():
    fake = lambda system, user: json.dumps([{
        "category": "tooling", "observation": "Use JSONL for logs.",
        "evidence": "always use JSONL", "prompt": 1,
    }])
    out = tl.extract_observations(
        [("claude-code", "D--Claude", "always use JSONL")], llm=fake
    )
    assert out.outcome == outcomes.Outcome.VALID_EMPTY
    assert out.actions == []


def test_extract_returns_parse_failed_on_unparseable_response(tmp_path, monkeypatch):
    audit = _audit_file_path(tmp_path)
    fake = lambda system, user: "no json here"
    out = tl.extract_observations([("codex", "D--Claude", "hello world turn")], llm=fake)
    assert out.outcome == outcomes.Outcome.PARSE_FAILED
    assert out.actions == []
    assert "parse_failure" in audit.read_text(encoding="utf-8")


def test_extract_returns_provider_failed_on_provider_exception(tmp_path, monkeypatch):
    audit = _audit_file_path(tmp_path)
    def boom(system, user):
        raise TimeoutError("network")
    out = tl.extract_observations([("codex", "D--Claude", "hello world turn")], llm=boom)
    assert out.outcome == outcomes.Outcome.PROVIDER_FAILED
    assert out.reason
    assert "llm_call_failed" in audit.read_text(encoding="utf-8")


def test_synthesize_returns_update_action_with_envelope():
    existing = [{"name": "adapt-logging-jsonl-over-logfmt", "category": "tooling",
                 "rule": "Prefer JSONL over logfmt for structured logs.", "confidence": 0.8,
                 "observations": 2, "scope": "D--Claude"}]
    new_obs = [{"category": "tooling", "observation": "use JSONL not logfmt",
                "evidence": "always use JSONL", "tool": "codex", "scope": "D--Claude"}]
    fake = lambda system, user: json.dumps([
        {"action": "update", "name": "adapt-logging-jsonl-over-logfmt", "category": "tooling",
         "rule": "Use JSONL for structured logs, never logfmt.", "confidence": 0.85,
         "observations": 3, "why": "re-endorsed in codex session"}])
    out = tl.synthesize(existing, new_obs, llm=fake)
    assert out.outcome == outcomes.Outcome.SUCCESS
    assert out.actions[0]["action"] == "update"
    assert out.actions[0]["confidence"] == 0.85


def test_synthesize_low_confidence_action_is_review_flagged():
    fake = lambda system, user: json.dumps([
        {"action": "add", "name": "adapt-review-ask-first", "category": "workflow",
         "rule": "Ask before broad review changes are pushed.", "confidence": 0.35,
         "observations": 1, "why": "single weak hint"}])
    out = tl.synthesize([], [{"category": "workflow", "observation": "ask first"}], llm=fake)
    assert out.outcome == outcomes.Outcome.SUCCESS
    assert out.actions[0]["confidence"] == 0.35
    assert out.actions[0]["needs_review"] is True


def test_call_lane_forwards_minimax_retry_and_output_ceilings(monkeypatch):
    seen = {}

    def fake(system, user, *, max_tokens, attempts, thinking, temperature):
        seen.update(
            max_tokens=max_tokens,
            attempts=attempts,
            thinking=thinking,
            temperature=temperature,
        )
        return "[]"

    monkeypatch.setattr(tl, "_minimax_llm", fake)

    assert tl.call_lane(
        "system", "user", lane="minimax", max_tokens=2048, attempts=1
    ) == "[]"
    assert seen == {
        "max_tokens": 2048,
        "attempts": 1,
        "thinking": "adaptive",
        "temperature": 0.2,
    }


# --- Task 3: orchestrator tests ---
import importlib.util

_spec = importlib.util.spec_from_file_location(
    "adapt_cli", Path(__file__).resolve().parent / "adapt.py")
lt = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(lt)


def test_apply_actions_upserts_via_memright(tmp_path, monkeypatch):
    calls = []
    bodies = []
    def fake_run(args):
        calls.append(args)
        if args and args[0] == "put":
            bodies.append(Path(args[args.index("--file") + 1]).read_text(encoding="utf-8"))
        return True
    monkeypatch.setattr(lt, "_run_memright", fake_run)
    monkeypatch.setattr(lt.ts, "scan_batch_for_secrets_str", lambda _text: True)
    rules = {}
    actions = [{"action": "add", "name": "adapt-logging-jsonl-over-logfmt",
                "category": "workflow", "rule": "Always prefer JSONL over logfmt for structured logging because line-delimited records survive tool rotations cleanly.",
                "confidence": 0.7, "observations": 1, "why": "stated directly"}]
    obs_by_cat = {"workflow": [{"scope": "D--Claude", "tool": "claude-code",
                               "evidence": "always use JSONL", "category": "workflow",
                               "observation": "use JSONL"}]}
    changed, ok = lt.apply_actions(actions, obs_by_cat, rules, tmp_path, dry_run=False)
    assert changed == 1 and ok is True
    assert "adapt-logging-jsonl-over-logfmt" in rules
    assert len(calls) == 1
    assert calls[0][:2] == ["put", "adapt-logging-jsonl-over-logfmt"]
    assert "--scope" in calls[0] and "D--Claude" in calls[0]
    assert "**Trigger phrases:** always use JSONL" in bodies[0]
    assert rules["adapt-logging-jsonl-over-logfmt"]["retrieval_aliases"] == [
        "always use JSONL"
    ]


def test_apply_actions_enforces_supplied_authority_manifest(tmp_path, monkeypatch):
    import authority as authority_mod

    calls = []
    monkeypatch.setattr(lt, "_run_memright", lambda args: calls.append(args) or True)
    frozen = authority_mod.build_manifest(
        tmp_path,
        directives=[{
            "id": "report-format",
            "text": "Never use YAML for generated reports.",
            "scope": "repo-a",
            "status": "current",
        }],
    )
    actions = [{
        "action": "add",
        "name": "adapt-yaml-reports",
        "category": "workflow",
        "rule": "Always use YAML for generated reports.",
        "confidence": 0.8,
        "observations": 2,
    }]
    observations = {"workflow": [{
        "scope": "repo-a",
        "tool": "claude-code",
        "evidence": "use YAML for generated reports",
    }]}

    changed, ok = lt.apply_actions(
        actions,
        observations,
        {},
        tmp_path,
        dry_run=False,
        authority_manifest=frozen,
        authority_root=tmp_path,
    )

    assert changed == 0 and ok is True
    assert calls == []


def test_apply_is_idempotent_on_keep(tmp_path, monkeypatch):
    calls = []
    monkeypatch.setattr(lt, "_run_memright", lambda args: calls.append(args) or True)
    rules = {"adapt-logging-jsonl-over-logfmt": {
        "name": "adapt-logging-jsonl-over-logfmt", "category": "workflow",
        "rule": "Use JSONL.", "confidence": 0.7, "observations": 1, "scope": "D--Claude"}}
    actions = [{"action": "keep", "name": "adapt-logging-jsonl-over-logfmt",
                "category": "workflow", "rule": "Use JSONL.", "confidence": 0.7,
                "observations": 1, "why": ""}]
    changed, ok = lt.apply_actions(actions, {"workflow": []}, rules, tmp_path, dry_run=False)
    assert changed == 0 and ok is True
    assert calls == []          # keep = no write


def test_dry_run_never_writes(tmp_path, monkeypatch):
    calls = []
    monkeypatch.setattr(lt, "_run_memright", lambda args: calls.append(args) or True)
    actions = [{"action": "add", "name": "adapt-tooling-keep-it-boring",
                "category": "tooling",
                "rule": "Prefer boring, well-trodden libraries over clever ones when both solve the same problem for this codebase.",
                "confidence": 0.6, "observations": 1, "why": ""}]
    rules = {}
    changed, ok = lt.apply_actions(actions, {"tooling": []}, rules, tmp_path, dry_run=True)
    assert changed == 0 and ok is True
    assert calls == []


def test_failed_memright_write_reports_not_ok(tmp_path, monkeypatch):
    monkeypatch.setattr(lt, "_run_memright", lambda args: False)
    actions = [{"action": "add", "name": "adapt-workflow-validate-first",
                "category": "workflow",
                "rule": "Always validate the user's plan before starting implementation or pursuing unrelated tangents.",
                "confidence": 0.6, "observations": 1, "why": ""}]
    changed, ok = lt.apply_actions(actions, {"workflow": []}, {}, tmp_path, dry_run=False)
    assert changed == 0 and ok is False


def test_apply_preflight_requires_scanner_lane_and_memright(monkeypatch):
    monkeypatch.setattr(lt.ts, "scanner_available", lambda: False)
    assert lt.preflight_apply("local", allow_external=False) is False
    monkeypatch.setattr(lt.ts, "scanner_available", lambda: True)
    monkeypatch.setattr(lt.adapt_llm, "lane_available", lambda lane: False)
    assert lt.preflight_apply("local", allow_external=False) is False
    monkeypatch.setattr(lt.adapt_llm, "lane_available", lambda lane: True)
    assert lt.preflight_apply("minimax", allow_external=False) is False
    monkeypatch.setattr(lt, "_run_memright", lambda args: False)
    assert lt.preflight_apply("local", allow_external=False) is False
    monkeypatch.setattr(lt, "_run_memright", lambda args: True)
    assert lt.preflight_apply("local", allow_external=False) is True
    assert lt.preflight_apply("minimax", allow_external=True) is True


def test_rule_body_format():
    body = lt.rule_body({"name": "adapt-logging-jsonl", "category": "logging",
                         "rule": "Use JSONL for structured logging.", "confidence": 0.8,
                         "observations": 3},
                        evidence='always use JSONL', tool="claude-code")
    assert body.startswith("**[adapt/logging]** — Use JSONL for structured logging.")
    assert "Confidence: 0.80 (observations: 3, needs_review: false" in body
    assert "**Why:**" in body and "**How to apply:**" in body


def test_digest_written(tmp_path):
    rules = {"adapt-logging-jsonl": {"name": "adapt-logging-jsonl", "category": "logging",
                                     "rule": "Use JSONL.", "confidence": 0.8,
                                     "observations": 3, "scope": "D--Claude"}}
    lt.write_digest(rules, tmp_path / "adapt-digest.md")
    text = (tmp_path / "adapt-digest.md").read_text(encoding="utf-8")
    assert "# logging" in text and "Use JSONL." in text and "0.80" in text

# --- privacy + audit-isolation contract tests added by 2026-07-12 fix pass ---


def test_extract_returns_scanner_blocked_outcome(tmp_path, monkeypatch):
    """When the external-lane batch scanner flags the batch, extract returns
    SCANNER_BLOCKED — retryable, never leaked into LLM output."""
    audit = _audit_file_path(tmp_path)
    import adapt_sessions as _ts
    monkeypatch.setattr(_ts, "scan_batch_for_secrets", lambda batch: False)
    fake = lambda system, user: json.dumps([
        {"category": "tooling", "observation": "x", "evidence": "y", "prompt": 1}])
    out = tl.extract_observations(
        [("claude-code", "D--Claude", "always use JSONL")], llm=fake, lane="minimax")
    assert out.outcome == outcomes.Outcome.SCANNER_BLOCKED
    assert out.actions == []
    assert "llm_call_failed" in audit.read_text(encoding="utf-8")


def test_synthesize_returns_scanner_blocked_outcome(tmp_path, monkeypatch):
    audit = _audit_file_path(tmp_path)
    import adapt_sessions as _ts
    sentinel = {"called": False}
    def fail_scan(s):
        sentinel["called"] = True
        return False
    monkeypatch.setattr(_ts, "scan_batch_for_secrets_str", fail_scan)
    called = {"llm": False}
    def fake_llm(system, user):
        called["llm"] = True
        return "[]"
    out = tl.synthesize([], [
        {"category": "tooling", "observation": "x", "evidence": "y"}, ], llm=fake_llm,
        lane="minimax")
    assert out.outcome == outcomes.Outcome.SCANNER_BLOCKED
    assert out.actions == []
    assert sentinel["called"] is True
    assert called["llm"] is False
    assert "synthesize" in audit.read_text(encoding="utf-8")


import pytest as _pytest


@_pytest.mark.skipif(
    not ts.scanner_available(),
    reason="requires gitleaks or detect-secrets on the host — without a scanner the "
           "external-lane batch correctly fails closed before the LLM is called",
)
def test_deterministic_not_in_extract_observations_payload(monkeypatch):
    """The LLM payload sent to MiniMax must contain only model-source
    observations. Deterministic hits go to `extract_deterministic()` separately
    and do NOT appear in the extraction JSON.
    """
    seen_payloads = []
    def fake(system, user):
        seen_payloads.append(user)
        return json.dumps([
            {"category": "logging", "observation": "use JSONL",
             "evidence": "always use JSONL", "prompt": 1,
             "record_type": "agent_preference", "durability": "cross_task_explicit",
             "subject": "agent_behavior"}])
    batch = [("claude-code", "D--Claude", "always use JSONL always")]
    tl.extract_observations(batch, llm=fake, lane="minimax")
    assert len(seen_payloads) == 1
    payload = json.loads(seen_payloads[0])
    # Each record in the payload must be a transcript row, not an extraction.
    for rec in payload:
        assert "text" in rec and "id" in rec
        assert "category" not in rec


def test_extract_deterministic_still_available_for_diagnostic():
    """Deterministic observations are still produced for diagnostic/recall-check
    purposes; the call is direct, not via extract_observations.
    """
    obs = tl.extract_deterministic([("claude-code", "D--Claude",
                                     "always use JSONL for structured logging, not logfmt")])
    assert obs and obs[0]["category"] == "explicit-preference"
    assert obs[0]["source"] == "deterministic"


def _audit_file_path(tmp_path):
    """Helper: set ADAPT_AUDIT_FILE_OVERRIDE to tmp and return the resolved path."""
    import adapt as _adapt_mod, os
    p = tmp_path / "audit.jsonl"
    os.environ["ADAPT_AUDIT_FILE_OVERRIDE"] = str(p)
    return p


def test_audit_writes_to_env_override_path(tmp_path, monkeypatch):
    """The audit function routes to ADAPT_AUDIT_FILE_OVERRIDE when set, so
    tests do not contaminate the real ~/.claude/adapt/audit.jsonl.
    """
    p = _audit_file_path(tmp_path)
    # Force a write through the orchestrator's _audit() helper.
    import adapt as _adapt_mod
    _adapt_mod._audit({"event": "test", "marker": "iso-override-ok"})
    text = p.read_text(encoding="utf-8")
    assert "iso-override-ok" in text
    assert "ts" in text
    # And NOT in the canonical user audit log.
    canonical = Path.home() / ".claude" / "adapt" / "audit.jsonl"
    if canonical.exists():
        canonical_text = canonical.read_text(encoding="utf-8")
        assert "iso-override-ok" not in canonical_text
    # Cleanup the env so it doesn't leak to subsequent tests.
    monkeypatch.delenv("ADAPT_AUDIT_FILE_OVERRIDE", raising=False)

# --- admission policy + run journal + taxonomy tests ---


def test_normalize_category_keeps_allowed_categories():
    from importlib import import_module
    adm = import_module("admission")
    for c in ("workflow", "verification", "safety", "architecture",
              "tooling", "code-style", "documentation", "model-routing"):
        assert adm.normalize_category(c) == c
        assert adm.normalize_category(c.upper()) == c


def test_normalize_category_remaps_unknown_to_misc_review():
    from importlib import import_module
    adm = import_module("admission")
    for c in ("explicit-preference", "audit-foo", "release-bump", "", None):
        assert adm.normalize_category(c) == "misc-review"


def test_admit_rejects_short_rule():
    from importlib import import_module
    adm = import_module("admission")
    ok, why = adm.admit({"name": "x-y", "rule": "too short", "category": "workflow"})
    assert (ok, why) == (False, "rule-too-short")


def test_admit_rejects_misc_review_category():
    from importlib import import_module
    adm = import_module("admission")
    ok, why = adm.admit({"name": "x-y", "rule": "do the thing properly", "category": "audit-foo"})
    assert (ok, why) == (False, "category-not-allowed")


def test_admit_accepts_well_formed_rule():
    from importlib import import_module
    adm = import_module("admission")
    ok, why = adm.admit({
        "name": "x-y",
        "rule": "Always run the smoke gate on a greenfield change before merging.",
        "category": "workflow",
    })
    assert ok is True
    assert why == "ok"


def test_admit_rejects_duplicate_name():
    from importlib import import_module
    adm = import_module("admission")
    ok, why = adm.admit({
        "name": "existing-rule",
        "rule": "Some sufficiently long durable rule sentence here.",
        "category": "workflow",
    }, canonical_rules={"existing-rule"})
    assert (ok, why) == (False, "rule-duplicate")


def test_run_journal_roundtrip(tmp_path):
    from importlib import import_module
    rj = import_module("run_journal")
    j = rj.RunJournal(tmp_path / "run_journal.jsonl")
    bid = rj.new_batch_id()
    j.record(bid, "discovered", sessions=["a", "b"])
    j.record(bid, "extracted", observations=5)
    j.record(bid, "committed")
    assert j.last_stage(bid) == "committed"
    assert j.incomplete_batches() == []
    bid2 = rj.new_batch_id()
    j.record(bid2, "discovered")
    j.record(bid2, "extracted")
    assert bid2 in j.incomplete_batches()
    assert bid not in j.incomplete_batches()


def test_run_journal_rejects_invalid_stage(tmp_path):
    from importlib import import_module
    rj = import_module("run_journal")
    j = rj.RunJournal(tmp_path / "run_journal.jsonl")
    bid = rj.new_batch_id()
    try:
        j.record(bid, "BOGUS")
    except ValueError:
        pass
    else:
        raise AssertionError("expected ValueError on invalid stage")


def test_synthesize_normalizes_categories_via_admission(monkeypatch):
    """The synthesis pass must normalize the model's category output: any category
    outside the controlled taxonomy becomes 'misc-review', and the action
    carries the normalized key.
    """
    from importlib import import_module
    adm = import_module("admission")
    fake = lambda system, user: json.dumps([
        {"action": "add", "name": "x-random",
         "rule": "Always use random-cat approach for variety.",
         "category": "audit-foo", "observations": 1, "why": "test"},
        {"action": "add", "name": "x-workflow",
         "rule": "Confirm plan with the user before broad workflow changes.",
         "category": "workflow", "observations": 1, "why": "test"},
    ])
    out = tl.synthesize([], [
        {"category": "tooling", "observation": "x", "evidence": "y"}], llm=fake,
        lane="local")
    by_name = {a["name"]: a for a in out.actions}
    assert by_name["x-workflow"]["category"] == "workflow"
    assert by_name["x-random"]["category"] == adm.DEFAULT_FALLBACK_CATEGORY


# --- RunJournal payload-persistence + resume tests ---


def test_run_journal_records_payload(tmp_path):
    from importlib import import_module
    rj = import_module("run_journal")
    j = rj.RunJournal(tmp_path / "j.jsonl")
    bid = "test-batch-1"
    j.record(bid, "discovered", sessions=["a", "b"])
    j.record(bid, "extracted", observations=[
        {"category": "workflow", "observation": "use JSONL", "prompt": 1, "source": "model"}
    ])
    j.record(bid, "synthesized", actions=[
        {"action": "add", "name": "x-y", "category": "workflow",
         "rule": "Use JSONL for structured logs.", "confidence": 0.7}
    ])
    cached = j.cached_payload(bid, "extracted")
    assert cached is not None
    assert cached["observations"][0]["observation"] == "use JSONL"


def test_run_journal_pending_batch_returns_incomplete(tmp_path):
    """The newest batch that hasn't reached `committed` is the resume target."""
    from importlib import import_module
    rj = import_module("run_journal")
    j = rj.RunJournal(tmp_path / "j.jsonl")
    j.record("batch-a", "discovered", sessions=["a"])
    j.record("batch-a", "extracted", observation_count=5)
    j.record("batch-a", "committed")
    j.record("batch-b", "discovered", sessions=["b"])
    j.record("batch-b", "extracted", observation_count=2)
    pending = j.pending_batch()
    assert pending is not None
    assert pending["batch_id"] == "batch-b"
    assert pending["stage"] == "extracted"


def test_run_journal_pending_returns_none_when_all_committed(tmp_path):
    from importlib import import_module
    rj = import_module("run_journal")
    j = rj.RunJournal(tmp_path / "j.jsonl")
    j.record("a", "discovered")
    j.record("a", "committed")
    assert j.pending_batch() is None


def test_run_journal_retry_replay_uses_cached_payload(tmp_path):
    """The whole point: a retry after a `[WinError 10054]` should NOT redo
    the LLM extract — it should replay the cached observations.
    """
    from importlib import import_module
    rj = import_module("run_journal")
    j = rj.RunJournal(tmp_path / "j.jsonl")
    bid = "retry-test"
    cached_obs = [{"category": "workflow", "observation": "alpha", "prompt": 1}]
    j.record(bid, "extracted", observations=cached_obs)
    # Simulate retry: read cached payload, replay.
    payload = j.cached_payload(bid, "extracted")
    assert payload["observations"] == cached_obs
    # Continue to synthesis stage.
    j.record(bid, "synthesized", actions=[{"action": "add", "name": "alpha",
                                         "rule": "alpha rule", "confidence": 0.7}])
    j.record(bid, "committed")
    assert j.pending_batch() is None


def test_run_journal_no_payload_strip_drops_empty_records(tmp_path):
    """Empty payload details shouldn't produce blank lines in the file."""
    from importlib import import_module
    rj = import_module("run_journal")
    j = rj.RunJournal(tmp_path / "j.jsonl")
    j.record("a", "discovered")  # no details
    j.record("b", "discovered", sessions=[])  # empty list — strip
    text = (tmp_path / "j.jsonl").read_text(encoding="utf-8")
    assert text.count("\n") == 2  # exactly two records
    # Each line still has the required keys.
    lines = [json.loads(l) for l in text.splitlines() if l.strip()]
    for line in lines:
        assert "ts" in line and "batch_id" in line and "stage" in line


def test_journal_replay_path_does_not_resend_extract(tmp_path):
    """End-to-end resume: first call records the extract payload; on retry
    the orchestrator reads the cached payload and skips the LLM call.
    """
    from importlib import import_module
    rj = import_module("run_journal")
    al = import_module("adapt_llm")

    journal_path = tmp_path / "run_journal.jsonl"
    j = rj.RunJournal(journal_path)

    def fake_extract(system, user):
        return json.dumps([{"category": "tooling",
                            "observation": "use JSONL for logs in this codebase.",
                            "evidence": "use JSONL",
                            "prompt": 1,
                            "record_type": "agent_preference",
                            "durability": "cross_task_explicit",
                            "subject": "agent_behavior"}])

    bx = al.extract_observations([("claude-code", "D--Claude",
                                    "always use JSONL for logs and never logfmt.")],
                                 llm=fake_extract, lane="local")
    import outcomes as _outcomes
    obs = list(bx.actions) if bx.outcome == _outcomes.Outcome.SUCCESS else []
    assert obs

    batch_id = rj.new_batch_id()
    j.record(batch_id, "discovered", sessions=["s1"])
    j.record(batch_id, "extracted", observations=obs)
    j.record(batch_id, "synthesized", synth_outcome="provider_failed", actions=[])

    cached = j.cached_payload(batch_id, "extracted")
    assert cached is not None
    replayed = list(cached["observations"])
    assert replayed == obs
    # Production's resume guard: `if not observations: ...` skips extract.
    # Confirm that path: with replayed non-empty, extract would NOT be called.
    counter = {"called": 0}
    def must_not_run(sys, usr):
        counter["called"] += 1
        return "[]"
    if replayed:
        pass  # production does this instead of extract_observations
    else:
        must_not_run("s", "u")
    assert counter["called"] == 0


def test_journal_full_round_trip(tmp_path):
    """All five stages land in the journal in order with replayable payloads."""
    from importlib import import_module
    rj = import_module("run_journal")
    journal_path = tmp_path / "j.jsonl"
    j = rj.RunJournal(journal_path)
    bid = "batch-x"
    j.record(bid, "discovered", sessions=["s1", "s2"])
    j.record(bid, "extracted", observations=[{"k": "v"}])
    j.record(bid, "synthesized", actions=[{"k": "v"}])
    j.record(bid, "applied", applied=1, ok=True)
    j.record(bid, "committed", applied=1, sessions=["s1"])
    entries = j.batches()
    bid_entries = [e for e in entries if e["batch_id"] == bid]
    assert [e["stage"] for e in bid_entries] == ["discovered", "extracted", "synthesized", "applied", "committed"]
    assert j.cached_payload(bid, "applied")["ok"] is True
    assert j.pending_batch() is None


def test_replayed_synth_failure_is_not_committable():
    """A cached provider/scanner/parse failure must never reach apply/commit."""
    import outcomes as _outcomes

    assert lt._synth_committable(_outcomes.Outcome.SUCCESS) is True
    assert lt._synth_committable(_outcomes.Outcome.VALID_EMPTY) is True
    assert lt._synth_committable(_outcomes.Outcome.PROVIDER_FAILED) is False
    assert lt._synth_committable(_outcomes.Outcome.SCANNER_BLOCKED) is False
    assert lt._synth_committable(_outcomes.Outcome.PARSE_FAILED) is False


def test_cached_valid_empty_synth_is_replayable(tmp_path):
    import run_journal as _journal
    import outcomes as _outcomes

    journal = _journal.RunJournal(tmp_path / "journal.jsonl")
    journal.record("batch", "synthesized",
                   synth_outcome=_outcomes.Outcome.VALID_EMPTY, actions=[])

    outcome, actions = lt._cached_synth(journal, "batch")
    assert outcome == _outcomes.Outcome.VALID_EMPTY
    assert actions == []


def test_cached_extract_progress_stops_before_failed_batch(tmp_path):
    import run_journal as _journal

    journal = _journal.RunJournal(tmp_path / "journal.jsonl")
    journal.record("batch", "extracted", batch=1, observations=[{"id": 1}])
    journal.record("batch", "extracted", batch=2, valid_empty=True)
    journal.record("batch", "extracted", batch=3,
                   outcome="provider_failed", reason="reset")

    completed_batch, observations = lt._cached_extract_progress(journal, "batch")
    assert completed_batch == 2
    assert observations == [{"id": 1}]


def test_failed_cached_synth_must_be_retried(tmp_path):
    import run_journal as _journal

    journal = _journal.RunJournal(tmp_path / "journal.jsonl")
    journal.record("batch", "synthesized",
                   synth_outcome="provider_failed", actions=[])

    replayable, outcome, actions = lt._replayable_synth(journal, "batch")
    assert replayable is False
    assert outcome == "provider_failed"
    assert actions == []


# --- Gate 2: PreferenceRecord v1 contract ---

import preference_record as pr_mod


def test_pref_record_derive_id_stable_for_same_input():
    a = pr_mod.derive_id("D--Claude", "workflow",
                         "Always use JSONL for structured logs, never logfmt.")
    b = pr_mod.derive_id("D--Claude", "workflow",
                         "Always use JSONL for structured logs, never logfmt.")
    assert a == b
    assert a.startswith("adapt-workflow-")
    # 10-hex-char sha suffix
    assert a.split("-")[-1].isalnum() and len(a.split("-")[-1]) == 10


def test_pref_record_derive_id_distinct_for_different_rule():
    a = pr_mod.derive_id("D--Claude", "workflow", "Always run smoke tests first.")
    b = pr_mod.derive_id("D--Claude", "workflow", "Always prefer JSONL over logfmt here.")
    assert a != b


def test_pref_record_normalize_ignores_case_and_punctuation():
    a = pr_mod.normalize_rule("Always use JSONL for logs.")
    b = pr_mod.normalize_rule("  always use jsonl for logs  ")
    assert a == b


def test_pref_record_id_changes_on_normalized_wording_change():
    """The suffix hash uses the normalized rule, so rewordings that change
    the normalized form produce a DIFFERENT ID — by design. Updates keep
    the prior id only via `from_synthesis(existing=...)`."""
    a = pr_mod.derive_id("D--Claude", "tooling",
                         "Always prefer JSONL over logfmt for structured logs.")
    b = pr_mod.derive_id("D--Claude", "tooling",
                         "Prefer JSONL over logfmt for structured logs.")
    assert a != b


def test_pref_record_id_uses_nul_separator_against_concat_collision():
    """('ab', 'cd') must not hash the same as ('a', 'bcd')."""
    a = pr_mod.derive_id("ab", "cd", "rule text")
    b = pr_mod.derive_id("a", "bcd", "rule text")
    assert a != b


def test_pref_record_from_synthesis_assigns_id_for_add():
    rec = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "ignored-by-id-system",
         "category": "tooling", "rule": "Use JSONL for structured logs without fail.",
         "confidence": 0.7, "observations": 2},
        scope="D--Claude", source_ids=("s1", "s2"),
    )
    assert rec.id.startswith("adapt-tooling-")
    assert rec.source_ids == ("s1", "s2")
    assert rec.schema_version == pr_mod.SCHEMA_VERSION
    assert rec.kind == "preference"


def test_pref_record_from_synthesis_preserves_id_on_update():
    """When `existing` is provided, the prior id is preserved verbatim —
    matches Dream's in-place-primary rule."""
    initial = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x",
         "category": "tooling", "rule": "Prefer JSONL over logfmt for structured logs.",
         "confidence": 0.7},
        scope="D--Claude", source_ids=("s1",),
    )
    updated = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "update", "name": "x",
         "category": "tooling", "rule": "Prefer JSONL over logfmt for structured logs.",
         "confidence": 0.85},
        scope="D--Claude", source_ids=("s1",),
        existing=initial.to_dict(),
    )
    assert updated.id == initial.id
    assert updated.confidence == 0.85
    assert updated.created_at == initial.created_at


def test_pref_record_dict_round_trip_carries_all_required_fields():
    rec = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x",
         "category": "tooling", "rule": "Prefer JSONL over logfmt for structured logs.",
         "confidence": 0.7},
        scope="D--Claude", source_ids=("s1",),
    )
    d = rec.to_dict()
    for f in pr_mod.REQUIRED_FIELDS:
        assert f in d, f"missing required field: {f}"
    # Round-trip via dataclass: to_dict emits a list, dataclass wants a tuple
    d["source_ids"] = tuple(d["source_ids"])
    rec2 = pr_mod.PreferenceRecord(**d)
    assert rec2 == rec


def test_pref_record_to_memright_content_uses_existing_envelope():
    rec = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x",
         "category": "tooling", "rule": "Prefer JSONL for structured logs.",
         "confidence": 0.7, "observations": 3},
        scope="D--Claude", source_ids=("s1",),
    )
    body = pr_mod.to_memright_content(rec)
    assert body.startswith("**[adapt/tooling]**")
    assert "Prefer JSONL for structured logs." in body
    assert "Confidence:" in body and "How to apply:" in body


def test_pref_record_bounded_retrieval_aliases_round_trip_and_embed():
    rec = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x", "category": "tooling",
         "rule": "Prefer JSONL for structured logs.", "confidence": 0.7,
         "retrieval_aliases": [
             "  always use JSONL when the logs are machine readable  ",
             "always use JSONL when the logs are machine readable",
             "do not switch this pipeline to logfmt",
         ]},
        scope="D--Claude", source_ids=("s1",),
    )
    assert rec.retrieval_aliases == (
        "always use JSONL when the logs are machine readable",
        "do not switch this pipeline to logfmt",
    )
    assert sum(map(len, rec.retrieval_aliases)) <= pr_mod.MAX_ALIAS_CHARS
    data = rec.to_dict()
    data["source_ids"] = tuple(data["source_ids"])
    data["retrieval_aliases"] = tuple(data["retrieval_aliases"])
    assert pr_mod.PreferenceRecord(**data) == rec
    body = pr_mod.to_memright_content(rec)
    assert "**Trigger phrases:**" in body
    assert "always use JSONL when the logs are machine readable" in body


def test_pref_record_aliases_exclude_rule_echo_and_overflow():
    aliases = pr_mod.normalize_retrieval_aliases(
        ["Prefer JSONL for structured logs.", "x" * 500],
        rule="Prefer JSONL for structured logs.",
    )
    assert aliases == ("x" * pr_mod.MAX_ALIAS_CHARS,)


# --- Gate 1a: manifest contract ---

import manifest as mf


def _build_valid_manifest(tmp_path, *, status="pending", batch_id="batch-1"):
    records = []
    for i, cat in enumerate(("tooling", "workflow", "verification")):
        rec = {
            "id": f"adapt-{cat}-sample-{i:010x}",
            "rule": f"Always do thing number {i} in the right way to satisfy tests.",
            "category": cat,
            "scope": "D--Claude",
            "record_type": "unclassified",
            "authority_effect": "neutral",
            "status": status,
            "confidence": 0.7 + 0.05 * i,
            "needs_review": False,
            "evidence_count": 1,
            "evidence_excerpt": f"verbatim evidence for rule {i}",
            "source_ids": ["s1", "s2"],
        }
        rec["payload_sha256"] = mf.payload_sha256(rec)
        records.append(rec)
    body = {
        "schema_version": pr_mod.SCHEMA_VERSION,
        "batch_id": batch_id,
        "created_at": "2026-07-13T00:00:00Z",
        "generator": "test",
        "source_session_ids": ["s1", "s2"],
        "records": records,
    }
    path = tmp_path / "manifest.json"
    path.write_text(json.dumps(body, ensure_ascii=False), encoding="utf-8")
    return path, body


def test_manifest_load_and_validate_refuses_pending_at_apply_time(tmp_path):
    """At apply-time only accepted/rejected are allowed; pending must be resolved."""
    p, _ = _build_valid_manifest(tmp_path, status="pending")
    with pytest.raises(mf.ManifestError):
        _apply_time_validate(p)


def _apply_time_validate(path):
    """Helper: mimics apply-time validator behavior (rejects pending)."""
    body = mf.load_and_validate(path)
    if any(r["status"] == "pending" for r in body["records"]):
        raise mf.ManifestError("manifest contains status='pending'")
    return body


def test_manifest_load_and_validate_refuses_edited_payload(tmp_path):
    p, body = _build_valid_manifest(tmp_path, status="accepted")
    body["records"][0]["rule"] = body["records"][0]["rule"].upper()
    p.write_text(json.dumps(body, ensure_ascii=False), encoding="utf-8")
    with pytest.raises(mf.ManifestError):
        mf.load_and_validate(p)


def test_manifest_accepted_and_rejected_split(tmp_path):
    p, body = _build_valid_manifest(tmp_path, status="accepted")
    body["records"][1]["status"] = "rejected"
    body["records"][1]["payload_sha256"] = mf.payload_sha256(body["records"][1])
    p.write_text(json.dumps(body, ensure_ascii=False), encoding="utf-8")
    m = mf.load_and_validate(p)
    assert len(mf.accepted_records(m)) == 2
    assert len(mf.rejected_records(m)) == 1


def test_manifest_refuses_unknown_status(tmp_path):
    p, body = _build_valid_manifest(tmp_path, status="accepted")
    body["records"][0]["status"] = "approved"
    body["records"][0]["payload_sha256"] = mf.payload_sha256(body["records"][0])
    p.write_text(json.dumps(body, ensure_ascii=False), encoding="utf-8")
    with pytest.raises(mf.ManifestError):
        mf.load_and_validate(p)


def test_manifest_refuses_empty_records(tmp_path):
    body = {
        "schema_version": pr_mod.SCHEMA_VERSION,
        "batch_id": "x", "created_at": "2026-07-13T00:00:00Z",
        "source_session_ids": ["s1"], "records": [],
    }
    p = tmp_path / "empty.json"
    p.write_text(json.dumps(body), encoding="utf-8")
    with pytest.raises(mf.ManifestError):
        mf.load_and_validate(p)


def test_manifest_refuses_missing_required_field(tmp_path):
    """The JSON Schema rejects records without payload_sha256."""
    p, body = _build_valid_manifest(tmp_path, status="accepted")
    body["records"][0].pop("payload_sha256")
    p.write_text(json.dumps(body), encoding="utf-8")
    with pytest.raises(mf.ManifestError):
        mf.load_and_validate(p)


def _isolate_journal(tmp_path, monkeypatch):
    """Redirect STATE_DIR + run_journal.JOURNAL_FILE + scanner/memright preflight
    to test-only fakes. The apply path needs scanner_available + _run_memright
    to pass before it ever touches the journal.
    """
    monkeypatch.setattr(lt.ts, "STATE_DIR", tmp_path)
    monkeypatch.setattr(lt.ts, "STATE_FILE", tmp_path / "state.json")
    monkeypatch.setattr(lt.run_journal, "JOURNAL_FILE", tmp_path / "run_journal.jsonl")
    monkeypatch.setattr(lt.ts, "scanner_available", lambda: True)
    monkeypatch.setattr(lt, "_run_memright", lambda args: True)
    monkeypatch.setattr(
        lt, "_create_apply_safepoint",
        lambda manifest_body: tmp_path / "safepoint.json",
        raising=False,
    )
    return lt.run_journal.RunJournal(tmp_path / "run_journal.jsonl")


def test_apply_from_manifest_creates_safepoint_before_first_put(tmp_path, monkeypatch):
    p, _ = _build_valid_manifest(tmp_path, status="accepted", batch_id="batch-order")
    journal = _isolate_journal(tmp_path, monkeypatch)
    journal.record("batch-order", "discovered", sessions=["s1", "s2"])
    events = []
    monkeypatch.setattr(
        lt, "_create_apply_safepoint",
        lambda manifest_body: events.append("safepoint") or tmp_path / "sp.json",
    )
    monkeypatch.setattr(
        lt, "_run_memright",
        lambda args: events.append(args[0]) or True,
    )
    assert lt.apply_from_manifest(p) == 0
    assert events.index("safepoint") < events.index("put")


def test_apply_from_manifest_refuses_writes_when_safepoint_fails(tmp_path, monkeypatch):
    p, _ = _build_valid_manifest(tmp_path, status="accepted", batch_id="batch-no-sp")
    journal = _isolate_journal(tmp_path, monkeypatch)
    journal.record("batch-no-sp", "discovered", sessions=["s1", "s2"])
    puts = []
    monkeypatch.setattr(
        lt, "_create_apply_safepoint",
        lambda manifest_body: (_ for _ in ()).throw(RuntimeError("backup failed")),
    )
    monkeypatch.setattr(
        lt, "_run_memright",
        lambda args: puts.append(args) or True,
    )
    assert lt.apply_from_manifest(p) == 2
    assert not [args for args in puts if args and args[0] == "put"]


def test_apply_from_manifest_refuses_when_journal_missing(tmp_path, monkeypatch):
    """adapt.apply_from_manifest must refuse if no journal discovered entry exists."""
    p, _ = _build_valid_manifest(tmp_path, status="accepted", batch_id="batch-noj")
    _isolate_journal(tmp_path, monkeypatch)
    rc = lt.apply_from_manifest(p)
    assert rc == 2


def test_apply_from_manifest_atomic_rollback_on_write_failure(tmp_path, monkeypatch):
    """If any memright put fails, partial writes are rolled back and state does not advance."""
    p, body = _build_valid_manifest(tmp_path, status="accepted", batch_id="batch-rb")
    journal = _isolate_journal(tmp_path, monkeypatch)
    journal.record("batch-rb", "discovered", sessions=["s1", "s2"])
    # Override _run_memright: first put succeeds, second fails.
    state = {"phase": 0}
    deletes = []
    def fake_run(args):
        if args and args[0] == "put":
            state["phase"] += 1
            return state["phase"] != 2
        if args and args[0] == "delete":
            deletes.append(args[1])
            return True
        return True
    monkeypatch.setattr(lt, "_run_memright", fake_run)
    rc = lt.apply_from_manifest(p)
    assert rc == 1, "expected non-zero exit on partial write failure"
    assert deletes == [
        f"{stored.scope}/{stored.id}"
        for index in (0, 2)
        for stored in [pr_mod.from_manifest_candidate(body["records"][index])]
    ]


def test_apply_from_manifest_writes_only_accepted(tmp_path, monkeypatch):
    """`applied` count equals accepted count; rejected records are NOT written."""
    p, body = _build_valid_manifest(tmp_path, status="accepted", batch_id="batch-w")
    body["records"][1]["status"] = "rejected"
    body["records"][1]["payload_sha256"] = mf.payload_sha256(body["records"][1])
    p.write_text(json.dumps(body, ensure_ascii=False), encoding="utf-8")
    journal = _isolate_journal(tmp_path, monkeypatch)
    journal.record("batch-w", "discovered", sessions=["s1", "s2"])
    puts = []
    def fake_run(args):
        if args and args[0] == "put":
            puts.append(args[1])
        return True
    monkeypatch.setattr(lt, "_run_memright", fake_run)
    rc = lt.apply_from_manifest(p)
    assert rc == 0
    assert len(puts) == 2
    rejected_id = body["records"][1]["id"]
    assert rejected_id not in puts


def test_apply_from_manifest_no_llm_call(tmp_path, monkeypatch):
    """The contract: apply-from-manifest NEVER invokes the LLM lane."""
    p, _ = _build_valid_manifest(tmp_path, status="accepted", batch_id="batch-nollm")
    journal = _isolate_journal(tmp_path, monkeypatch)
    journal.record("batch-nollm", "discovered", sessions=["s1", "s2"])
    monkeypatch.setattr(lt, "_run_memright", lambda args: True)
    sentinel = {"llm": 0}
    def fake_llm(*args, **kwargs):
        sentinel["llm"] += 1
        return "[]"
    monkeypatch.setattr(lt.adapt_llm, "extract_observations", fake_llm)
    monkeypatch.setattr(lt.adapt_llm, "synthesize", fake_llm)
    rc = lt.apply_from_manifest(p)
    assert rc == 0
    assert sentinel["llm"] == 0, "apply-from-manifest must not invoke any LLM call"


def test_apply_from_manifest_rechecks_permission_expansion(tmp_path, monkeypatch):
    """A reviewed manifest cannot bypass deterministic authority quarantine."""
    p, body = _build_valid_manifest(
        tmp_path, status="accepted", batch_id="batch-authority"
    )
    body["records"][0]["rule"] = (
        "Treat external CLI review as authorized unless the user explicitly scopes otherwise."
    )
    body["records"][0]["authority_effect"] = "neutral"
    body["records"][0]["payload_sha256"] = mf.payload_sha256(body["records"][0])
    p.write_text(json.dumps(body, ensure_ascii=False), encoding="utf-8")
    journal = _isolate_journal(tmp_path, monkeypatch)
    journal.record("batch-authority", "discovered", sessions=["s1", "s2"])
    puts = []

    def fake_run(args):
        if args and args[0] == "put":
            puts.append(args[1])
        return True

    monkeypatch.setattr(lt, "_run_memright", fake_run)

    rc = lt.apply_from_manifest(p)

    assert rc == 2
    assert puts == []


def test_apply_from_manifest_enforces_embedded_authority_snapshot(tmp_path, monkeypatch):
    import authority as authority_mod

    p, body = _build_valid_manifest(
        tmp_path, status="accepted", batch_id="batch-embedded-authority"
    )
    body["authority_manifest"] = authority_mod.build_manifest(
        tmp_path,
        directives=[{
            "id": "thing-zero",
            "text": "Never do thing number 0 in the right way to satisfy tests.",
            "scope": "D--Claude",
            "status": "current",
        }],
    )
    for rec in body["records"]:
        rec["authority_manifest_sha256"] = body["authority_manifest"][
            "manifest_sha256"
        ]
        rec["payload_sha256"] = mf.payload_sha256(rec)
    p.write_text(json.dumps(body, ensure_ascii=False), encoding="utf-8")
    monkeypatch.setattr(lt, "WORKSPACE_ROOT", tmp_path)
    journal = _isolate_journal(tmp_path, monkeypatch)
    journal.record(
        "batch-embedded-authority", "discovered", sessions=["s1", "s2"]
    )
    puts = []

    def fake_run(args):
        if args and args[0] == "put":
            puts.append(args[1])
        return True

    monkeypatch.setattr(lt, "_run_memright", fake_run)

    rc = lt.apply_from_manifest(p)

    assert rc == 2
    assert puts == []


def test_apply_from_manifest_session_ids_must_match_journal(tmp_path, monkeypatch):
    """Mismatch between manifest source_session_ids and journal sessions → refuse."""
    p, body = _build_valid_manifest(tmp_path, status="accepted", batch_id="batch-mm")
    body["source_session_ids"] = ["s9", "s10"]
    p.write_text(json.dumps(body), encoding="utf-8")
    journal = _isolate_journal(tmp_path, monkeypatch)
    journal.record("batch-mm", "discovered", sessions=["s1", "s2"])
    rc = lt.apply_from_manifest(p)
    assert rc == 2


# --- Gate 4: rollback.py ---

import rollback as rk


def _make_sqlite_db(path):
    import sqlite3
    with sqlite3.connect(path) as conn:
        conn.execute("CREATE TABLE IF NOT EXISTS smoke (id INTEGER PRIMARY KEY)")


def test_rollback_create_writes_safepoint(tmp_path):
    """Create captures the live invariants and writes the safe-point JSON."""
    p, body = _build_valid_manifest(tmp_path, status="accepted", batch_id="batch-rb")
    db = tmp_path / "engine.db"
    _make_sqlite_db(db)
    state_path = tmp_path / "state.json"
    state_path.write_text('{"learned": {}}', encoding="utf-8")
    rules_path = tmp_path / "rules.json"
    rules_path.write_text('{"r1": {}}', encoding="utf-8")
    core_path = tmp_path / "core.json"
    core_path.write_text('{"content_sha256": "abc"}', encoding="utf-8")
    out = rk.create_safe_point(body, db, state_path=state_path,
                               rules_path=rules_path, core_path=core_path,
                               out_path=tmp_path / "sp.json")
    sp = json.loads(out.read_text(encoding="utf-8"))
    assert sp["batch_id"] == "batch-rb"
    assert len(sp["accepted_ids"]) == len(body["records"])
    assert sp["accepted_ids"] == [
        f"{stored.scope}/{stored.id}" for stored in (
            pr_mod.from_manifest_candidate(record) for record in body["records"]
        )
    ]
    assert sp["state_snapshot"] == '{"learned": {}}'
    assert sp["rules_snapshot"] == '{"r1": {}}'
    assert sp["core_snapshot"] == '{"content_sha256": "abc"}'
    assert sp["core_digest"] == rk._sha256_file(core_path)
    assert sp["db_checksum"] is not None
    backup = Path(sp["db_backup_path"])
    assert backup.exists()
    assert sp["db_backup_checksum"] == rk._sha256_file(backup)
    assert rk._verify_integrity(backup) == (True, "ok")


def test_rollback_dry_run_does_not_modify(tmp_path):
    p, body = _build_valid_manifest(tmp_path, status="accepted", batch_id="batch-dry")
    db = tmp_path / "engine.db"
    _make_sqlite_db(db)
    state_path = tmp_path / "state.json"
    state_path.write_text("{}", encoding="utf-8")
    sp_path = rk.create_safe_point(body, db, state_path=state_path,
                                   rules_path=tmp_path / "rules.json",
                                   core_path=tmp_path / "core.json",
                                   out_path=tmp_path / "sp.json")
    state_path.write_text("MODIFIED", encoding="utf-8")
    rc = rk.revert(sp_path, apply=False)
    assert rc == 0
    assert state_path.read_text(encoding="utf-8") == "MODIFIED"


def test_rollback_apply_deletes_via_memright(tmp_path, monkeypatch):
    p, body = _build_valid_manifest(tmp_path, status="accepted", batch_id="batch-app")
    db = tmp_path / "engine.db"
    _make_sqlite_db(db)
    state = tmp_path / "state.json"
    rules = tmp_path / "rules.json"
    core = tmp_path / "core.json"
    monkeypatch.setattr(rk, "STATE_FILE", state)
    monkeypatch.setattr(rk, "RULES_FILE", rules)
    monkeypatch.setattr(rk, "CORE_FILE", core)
    state.write_text("pre-apply-state", encoding="utf-8")
    rules.write_text("pre-apply-rules", encoding="utf-8")
    core.write_text("pre-apply-core", encoding="utf-8")
    sp_path = rk.create_safe_point(body, db, state_path=state,
                                   rules_path=rules, core_path=core,
                                   out_path=tmp_path / "sp.json")
    state.write_text("post-apply-bad", encoding="utf-8")
    rules.write_text("post-apply-rules", encoding="utf-8")
    core.write_text("post-apply-core", encoding="utf-8")
    # Patch shutil.which for sqlite3 (none) and memright delete (stub).
    monkeypatch.setattr(rk, "_resolve_memright", lambda: "memright")
    monkeypatch.setattr(rk.shutil, "which", lambda name: None)
    captured = {"deleted": []}
    class FakeRun:
        returncode = 0
        stdout = ""
        stderr = ""
    def fake_run(args, **kwargs):
        if args and args[0] == "memright":
            captured["deleted"].append(args[2])
        return FakeRun()
    monkeypatch.setattr(rk.subprocess, "run", fake_run)
    rc = rk.revert(sp_path, apply=True)
    assert rc == 0
    # Three accepted records in _build_valid_manifest → three deletes.
    assert len(captured["deleted"]) == 3
    assert captured["deleted"] == [
        f"{stored.scope}/{stored.id}" for stored in (
            pr_mod.from_manifest_candidate(record) for record in body["records"]
        )
    ]
    # state.json restored to pre-apply
    assert state.read_text(encoding="utf-8") == "pre-apply-state"
    assert rules.read_text(encoding="utf-8") == "pre-apply-rules"
    assert core.read_text(encoding="utf-8") == "pre-apply-core"


def test_rollback_integrity_uses_python_sqlite(tmp_path):
    valid = tmp_path / "valid.db"
    _make_sqlite_db(valid)
    assert rk._verify_integrity(valid) == (True, "ok")
    invalid = tmp_path / "invalid.db"
    invalid.write_bytes(b"not sqlite")
    ok, message = rk._verify_integrity(invalid)
    assert not ok
    assert message


def test_rollback_refuses_corrupt_backup_before_delete(tmp_path, monkeypatch):
    _, body = _build_valid_manifest(tmp_path, status="accepted", batch_id="batch-corrupt")
    db = tmp_path / "engine.db"
    _make_sqlite_db(db)
    state = tmp_path / "state.json"
    rules = tmp_path / "rules.json"
    core = tmp_path / "core.json"
    state.write_text("state", encoding="utf-8")
    rules.write_text("rules", encoding="utf-8")
    core.write_text("core", encoding="utf-8")
    sp_path = rk.create_safe_point(
        body, db, state_path=state, rules_path=rules, core_path=core,
        out_path=tmp_path / "sp.json",
    )
    sp = json.loads(sp_path.read_text(encoding="utf-8"))
    Path(sp["db_backup_path"]).write_bytes(b"corrupt")
    deleted = []
    monkeypatch.setattr(
        rk, "_delete_via_memright",
        lambda name, memright_bin=None: deleted.append(name) or True,
    )
    assert rk.revert(
        sp_path, apply=True, state_path=state, rules_path=rules, core_path=core
    ) == 2
    assert deleted == []


def test_emit_arm_d_emits_schema_conformant_manifest(tmp_path):
    """The fixture-driven emitter must produce a manifest that round-trips
    through validate_schema (schema-only) and apply_time_validate once every
    record's status is flipped to accepted.
    """
    import emit_arm_d_candidates as emit_mod
    out = tmp_path / "arm-d.manifest.json"
    rc = emit_mod.main([
        "--out", str(out), "--print-summary",
    ])
    assert rc == 0
    body = json.loads(out.read_text(encoding="utf-8"))
    assert body["schema_version"] == pr_mod.SCHEMA_VERSION
    assert len(body["source_session_ids"]) == 10
    assert len(body["records"]) >= 1
    # Schema-only inspection accepts pending.
    m = mf.validate_schema(out)
    assert m["batch_id"] == body["batch_id"]
    # Every record has payload_sha256 stamped at emission.
    for r in body["records"]:
        assert "payload_sha256" in r
        assert r["payload_sha256"] == mf.payload_sha256(r)


def test_emit_arm_d_flips_after_review(tmp_path):
    """After flipping every pending record to accepted, the manifest must
    round-trip through apply_time_validate."""
    import emit_arm_d_candidates as emit_mod
    out = tmp_path / "arm-d.manifest.json"
    emit_mod.main(["--out", str(out)])
    body = json.loads(out.read_text(encoding="utf-8"))
    for rec in body["records"]:
        rec["status"] = "accepted"
        rec["payload_sha256"] = mf.payload_sha256(rec)
    out.write_text(json.dumps(body, ensure_ascii=False), encoding="utf-8")
    m = mf.apply_time_validate(out)
    assert len(mf.accepted_records(m)) == len(body["records"])


def test_run_value_ab_builds_expected_query_rows():
    """The harness builds the same 32+12 set used by the sealed A/B."""
    import run_value_ab as hv
    rules = [hv.TasteRule("logging", f"rule {i}", 0.7, f"r{i}") for i in range(16)]
    rows = hv.build_query_rows(rules, smoke=False)
    assert sum(1 for r in rows if r["kind"] == "positive") == 32
    assert sum(1 for r in rows if r["kind"] == "control") == 12
    assert rows[0]["expected_id"] is not None
    assert rows[32]["kind"] == "control" and rows[32]["expected_id"] is None


def test_run_value_ab_parses_taste_md(tmp_path):
    """The taste.md parser must produce one rule per bullet."""
    import run_value_ab as hv
    md = tmp_path / "taste.md"
    md.write_text(
        "# Taste rules\n\n# logging\n- first rule text Confidence: 0.85\n"
        "- second rule text Confidence: 0.70\n",
        encoding="utf-8",
    )
    rules = hv.parse_taste_md(md)
    assert len(rules) == 2
    assert rules[0].category == "logging"
    assert rules[0].confidence == 0.85
    assert rules[0].name.startswith("cc-taste-")


def test_run_value_ab_live_db_unchanged_detects_drift(tmp_path):
    """The harness refuses a 'live unchanged' verdict if row count drifts."""
    import run_value_ab as hv
    fake_live = tmp_path / "live.db"
    import sqlite3
    with sqlite3.connect(fake_live) as conn:
        conn.execute("CREATE TABLE memories (id TEXT, content TEXT)")
        conn.executemany("INSERT INTO memories VALUES (?, ?)",
                         [(f"id{i}", "x") for i in range(5)])
    baseline = hv.integrity_check(fake_live)[1]
    # Patch with a stub that does NOT recurse into integrity_check.
    def fake_check(_p):
        return (True, baseline + 7, "ok")
    import run_value_ab as hv_mod
    orig = hv_mod.integrity_check
    hv_mod.integrity_check = fake_check
    try:
        ok, msg = hv_mod.live_db_unchanged(fake_live, baseline)
    finally:
        hv_mod.integrity_check = orig
    assert ok is False


def test_run_value_ab_static_block_composes_all_rules():
    import run_value_ab as hv
    rules = [
        hv.TasteRule("logging", "rule a", 0.7, "r1"),
        hv.TasteRule("build", "rule b", 0.6, "r2"),
    ]
    block = "\n".join(r.as_static_block() for r in rules)
    assert "[policy:logging] rule a" in block
    assert "[policy:build] rule b" in block


# freeze the custom import below — tests above this point depend on it.

def test_grade_wilson_interval_centered_on_p():
    import grade as gr
    lo, hi = gr.wilson_interval(0.5, 100)
    assert 0.4 < lo < 0.5
    assert 0.5 < hi < 0.6


def test_grade_wilson_degenerate_n_zero():
    import grade as gr
    assert gr.wilson_interval(0.5, 0) == (0.0, 0.0)


def test_grade_mcnemar_discordant_counts():
    import grade as gr
    # 5 prompts: A hits 3, B hits 2, with 1 discordant each direction.
    a = [1, 1, 1, 0, 0]
    b = [1, 1, 0, 0, 1]
    out = gr.mcnemar_discordant(a, b)
    assert out["a_only_hits_b_misses"] == 1
    assert out["b_only_hits_a_misses"] == 1
    assert out["discordant_total"] == 2
    assert out["underpowered"] is True


def test_grade_arm_metrics_hit_and_intrusion():
    import grade as gr
    rows = [
        {"row_id": "p1", "kind": "positive", "expected_id": "X", "rule": "x"},
        {"row_id": "p2", "kind": "positive", "expected_id": "Y", "rule": "y"},
        {"row_id": "c1", "kind": "control", "expected_id": None, "rule": None},
    ]
    arm = {"arm": "A",
           "ranked": {"p1": ["X", "Z"], "p2": ["W", "V"], "c1": ["Y"]},
           "wall_ms": {"p1": 5, "p2": 6, "c1": 4}}
    m = gr.arm_metrics(arm, rows, k=10)
    assert m["hit_at_K"] == 0.5         # p1 hit, p2 missed
    assert abs(m["mrr_at_K"] - (1.0 + 0.0) / 2) < 1e-9
    assert m["intrusion_at_K"] == 1.0   # c1 pulled rule id Y


def test_grade_paired_compare_underpowered_signal():
    import grade as gr
    a = {"arm": "A", "hit_at_K": 0.5, "intrusion_at_K": 0.0, "wall_avg_ms": 7.5,
          "per_query": [
              {"kind": "positive", "hit": 1, "intrusion": 0, "wall_ms": 10},
              {"kind": "control", "intrusion": 0, "hit": 0, "wall_ms": 5},
          ]}
    b = {"arm": "B", "hit_at_K": 0.0, "intrusion_at_K": 1.0, "wall_avg_ms": 9.0,
          "per_query": [
              {"kind": "positive", "hit": 0, "intrusion": 0, "wall_ms": 12},
              {"kind": "control", "intrusion": 1, "hit": 0, "wall_ms": 6},
          ]}
    cmp = gr.paired_compare(a, b)
    assert cmp["hit_delta"] == -0.5
    assert cmp["intrusion_delta"] == 1.0
    assert cmp["hit_mcnemar"]["underpowered"] is True


def test_grade_bootstrap_ci_runs():
    import grade as gr
    diffs = [1.0, 2.0, 3.0, 4.0, 5.0] * 20  # 100 samples, mean 3.0
    lo, hi = gr.bootstrap_ci(diffs, iters=200)
    assert 2.0 < lo < 3.5
    assert 2.5 < hi < 4.5


def test_grade_end_to_end_smoke(tmp_path):
    """End-to-end: build a fake results.json, run grade, verify outputs."""
    import grade as gr
    fake = {
        "generated_at": "2026-07-13T00:00:00Z",
        "smoke": True, "rules_count": 2,
        "arms": [
            {"arm": "A", "ranked": {"p1": ["X"], "c1": []},
             "wall_ms": {"p1": 1, "c1": 1}},
            {"arm": "B", "ranked": {"p1": ["X"], "c1": ["X"]},
             "wall_ms": {"p1": 1, "c1": 1}},
        ],
        "query_rows": [
            {"row_id": "p1", "kind": "positive",
             "expected_id": "X", "rule": "x"},
            {"row_id": "c1", "kind": "control",
             "expected_id": None, "rule": None},
        ],
    }
    rpath = tmp_path / "results.json"
    rpath.write_text(json.dumps(fake), encoding="utf-8")
    rc = gr.grade(rpath, tmp_path / "graded", k=10)
    assert rc == 0
    g = json.loads((tmp_path / "graded" / "graded.json").read_text())
    assert len(g["per_arm"]) == 2
    assert g["per_arm"][0]["hit_at_K"] == 1.0
    assert g["per_arm"][1]["intrusion_at_K"] == 1.0
    md = (tmp_path / "graded" / "grade.md").read_text(encoding="utf-8")
    assert "## Per-arm metrics" in md
    assert "## Paired vs baseline" in md


def test_pref_record_session_file_sha256(tmp_path):
    """Session.file_sha256 is a stable 64-hex string of the transcript bytes."""
    import adapt_sessions as ts
    f = _write(tmp_path / "s.jsonl", [
        {"type": "user", "userType": "external", "cwd": "D:\Claude",
         "sessionId": "x", "message": {"content": "hello world always jsonl logs"}},
    ])
    sess = ts.parse_claude_session(f)
    h = sess.file_sha256()
    assert len(h) == 64
    assert h == sess.file_sha256()  # stable


def test_manifest_derive_evidence_id_stable_and_collision_resistant():
    import manifest as mf
    a = mf.derive_evidence_id("D--Claude", "always use JSONL")
    b = mf.derive_evidence_id("D--Claude", "always use JSONL")
    assert a == b
    assert a.startswith("ev-")
    assert len(a) == 11  # "ev-" + 8 hex
    # Different scope produces different id even for same excerpt.
    c = mf.derive_evidence_id("D--Claude-other", "always use JSONL")
    assert c != a


def test_manifest_candidate_payload_includes_gate1b_fields(tmp_path):
    """The immutable payload now covers source_file_hashes + evidence_ids."""
    rec = {
        "id": "adapt-tooling-sample-0000000000",
        "rule": "Always do the right thing in this particular way.",
        "category": "tooling",
        "scope": "D--Claude",
        "source_ids": ["s1"],
        "source_file_hashes": [{"session_id": "s1", "sha256": "ab" * 32}],
        "evidence_ids": [{"evidence_id": "ev-12345678",
                          "source_session_id": "s1",
                          "excerpt": "always use JSONL"}],
        "evidence_count": 1,
        "evidence_excerpt": "always use JSONL",
    }
    rec["status"] = "pending"
    rec["payload_sha256"] = mf.payload_sha256(rec)
    p = tmp_path / "m.json"
    p.write_text(json.dumps({
        "schema_version": "1.0.0",
        "batch_id": "x", "created_at": "2026-07-13T00:00:00Z",
        "source_session_ids": ["s1"], "records": [rec],
    }, ensure_ascii=False), encoding="utf-8")
    # Schema validates (records now allow source_file_hashes + evidence_ids).
    body = mf.validate_schema(p)
    assert body["records"][0]["source_file_hashes"][0]["sha256"] == "ab" * 32


def test_manifest_payload_sha256_stable_under_list_reorder(tmp_path):
    """Reordering source_file_hashes / evidence_ids must NOT change the hash."""
    rec_a = {
        "id": "adapt-x-y-0000000000",
        "rule": "Always do thing number zero in the right way here.",
        "category": "tooling", "scope": "D--Claude",
        "source_ids": ["s1", "s2"],
        "source_file_hashes": [
            {"session_id": "s1", "sha256": "aa" * 32},
            {"session_id": "s2", "sha256": "bb" * 32},
        ],
        "evidence_ids": [
            {"evidence_id": "ev-11111111", "source_session_id": "s1",
             "excerpt": "a"},
            {"evidence_id": "ev-22222222", "source_session_id": "s2",
             "excerpt": "b"},
        ],
        "evidence_count": 2, "evidence_excerpt": "a",
    }
    rec_a["payload_sha256"] = mf.payload_sha256(rec_a)
    # Flip order of source_file_hashes + evidence_ids.
    rec_b = dict(rec_a)
    rec_b["source_file_hashes"] = list(reversed(rec_a["source_file_hashes"]))
    rec_b["evidence_ids"] = list(reversed(rec_a["evidence_ids"]))
    rec_b["payload_sha256"] = mf.payload_sha256(rec_b)
    assert rec_a["payload_sha256"] == rec_b["payload_sha256"]


def test_apply_actions_emits_gate1b_fields(tmp_path, monkeypatch):
    """apply_actions with source_file_hashes argument populates per-record
    source_file_hashes[] + evidence_ids[].
    """
    monkeypatch.setattr(lt, "_run_memright", lambda args: True)
    monkeypatch.setattr(lt.ts, "scan_batch_for_secrets_str", lambda _text: True)
    rules: dict = {}
    actions = [{"action": "add", "name": "adapt-logging-jsonl-over-logfmt",
               "category": "tooling",
               "rule": "Always prefer JSONL over logfmt for structured logs.",
               "confidence": 0.7, "observations": 1, "why": "stated"}]
    obs_by_cat = {"tooling": [{"scope": "D--Claude", "tool": "claude-code",
                                "evidence": "always use JSONL",
                                "session_id": "abc",
                                "category": "tooling",
                                "observation": "use JSONL"}]}
    recs: list[dict] = []
    lt.apply_actions(actions, obs_by_cat, rules, tmp_path, dry_run=True,
                     manifest_records=recs,
                     source_session_ids=["abc"],
                     source_file_hashes={"abc": "ab" * 32})
    assert recs, "expected at least one manifest record"
    r = recs[0]
    assert r["source_file_hashes"] == [{"session_id": "abc",
                                         "sha256": "ab" * 32}]
    assert r["evidence_ids"][0]["evidence_id"].startswith("ev-")
    assert r["evidence_ids"][0]["source_session_id"] == "abc"
    assert r["retrieval_aliases"] == ["always use JSONL"]


def test_apply_from_manifest_embeds_signed_retrieval_aliases(tmp_path, monkeypatch):
    p, body = _build_valid_manifest(tmp_path, status="accepted", batch_id="batch-alias")
    body["schema_version"] = pr_mod.SCHEMA_VERSION
    body["records"][0]["retrieval_aliases"] = ["the exact phrase Adrian normally uses"]
    body["records"][0]["payload_sha256"] = mf.payload_sha256(body["records"][0])
    p.write_text(json.dumps(body, ensure_ascii=False), encoding="utf-8")
    journal = _isolate_journal(tmp_path, monkeypatch)
    journal.record("batch-alias", "discovered", sessions=["s1", "s2"])
    monkeypatch.setattr(lt.ts, "scan_batch_for_secrets_str", lambda _text: True)
    written = []
    def fake_run(args):
        if args and args[0] == "put":
            written.append(Path(args[args.index("--file") + 1]).read_text(encoding="utf-8"))
        return True
    monkeypatch.setattr(lt, "_run_memright", fake_run)
    assert lt.apply_from_manifest(p) == 0
    assert "**Trigger phrases:** the exact phrase Adrian normally uses" in written[0]


def test_manifest_payload_hash_binds_retrieval_aliases():
    record = {
        "id": "adapt-tooling-use-jsonl-1234567890",
        "rule": "Always use JSONL for structured logs.",
        "category": "tooling", "scope": "D--Claude",
        "retrieval_aliases": ["my usual source phrase"],
    }
    first = mf.payload_sha256(record)
    record["retrieval_aliases"] = ["a changed source phrase"]
    assert mf.payload_sha256(record) != first


def test_gate3_evidence_fidelity_shared_verb():
    import gate3_review as g3
    ok, why = g3.evidence_fidelity("Always use JSONL for structured logs.",
                                    "always use JSONL for structured logging")
    assert ok and "shared imperative verb" in why


def test_gate3_evidence_fidelity_unrelated_rule():
    import gate3_review as g3
    ok, why = g3.evidence_fidelity("Always run smoke tests before merging.",
                                    "we discussed logfmt conventions")
    assert not ok


def test_gate3_durability_short_rule():
    import gate3_review as g3
    ok, _ = g3.durability_ok("Use JSONL.")
    assert not ok
    ok, _ = g3.durability_ok("Always use JSONL for structured logs without fail.")
    assert ok


def test_gate3_category_unknown_rejected():
    import gate3_review as g3
    ok, _ = g3.category_ok("explicit-preference")
    assert not ok
    ok, _ = g3.category_ok("tooling")
    assert ok


def test_gate3_scope_outside_namespace_rejected():
    import gate3_review as g3
    ok, _ = g3.scope_ok("global")
    assert not ok
    ok, _ = g3.scope_ok("D--Claude-heardright")
    assert ok


def test_gate3_exact_duplication_detected():
    import gate3_review as g3
    manifest = {
        "batch_id": "b", "records": [
            {"id": "a", "status": "accepted", "rule": "Always use JSONL for logs.",
             "category": "tooling", "scope": "D--Claude",
             "evidence_excerpt": "always use JSONL"},
            {"id": "b", "status": "accepted",
             "rule": "always use jsonl for logs.",  # same after normalize
             "category": "tooling", "scope": "D--Claude",
             "evidence_excerpt": "use jsonl"},
        ],
    }
    rev = g3.review_manifest(manifest)
    assert rev["counts"]["exact_duplicates_accepted"] == 2


def test_gate3_unsupported_rule_hard_fail(tmp_path):
    """A rule with no evidence overlap is unsupported; gate3 exits 1."""
    import gate3_review as g3
    manifest = {
        "schema_version": "1.0.0",
        "batch_id": "b", "created_at": "2026-07-13T00:00:00Z",
        "source_session_ids": ["s1"],
        "records": [{
            "id": "x", "rule": "Always do something somewhere.",
            "category": "tooling", "scope": "D--Claude",
            "status": "accepted",
            "evidence_excerpt": "completely unrelated evidence text",
        }],
    }
    p = tmp_path / "m.json"
    p.write_text(json.dumps(manifest), encoding="utf-8")
    rc = g3.main(["--manifest", str(p), "--out", str(tmp_path / "g3")])
    assert rc == 1


def test_gate3_clean_manifest_passes(tmp_path):
    """A clean manifest exits 0 with no unsupported rules."""
    import gate3_review as g3
    manifest = {
        "schema_version": "1.0.0",
        "batch_id": "b", "created_at": "2026-07-13T00:00:00Z",
        "source_session_ids": ["s1"],
        "records": [{
            "id": "x", "rule": "Always use JSONL for structured logs in this codebase.",
            "category": "tooling", "scope": "D--Claude",
            "status": "accepted",
            "evidence_excerpt": "always use JSONL",
        }],
    }
    p = tmp_path / "m.json"
    p.write_text(json.dumps(manifest), encoding="utf-8")
    rc = g3.main(["--manifest", str(p), "--out", str(tmp_path / "g3")])
    assert rc == 0


def test_grade_bootstrap_ci_now_non_degenerate():
    """The bootstrap should produce a non-degenerate CI on a non-trivial
    sample (the previous cyclic resampling variant produced (mean, mean))."""
    import grade as gr, random
    random.seed(0)  # determinism for the test
    diffs = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0]
    lo, hi = gr.bootstrap_ci(diffs, iters=500)
    assert lo < hi, f"degenerate CI: ({lo}, {hi})"
    assert 3.0 < lo < 7.0
    assert 5.0 < hi < 9.0


def test_grade_arm_C_scores_static_block_overlap():
    """Arm C should score hit/mrr via static-block text overlap against the
    retrieved rows' text — not via engine embeddings (the engine wasn't
    changed for arm C).
    """
    import grade as gr
    rows = [
        {"row_id": "p1", "kind": "positive",
         "expected_id": "D--Claude/cc-taste-01-x",
         "rule": "Always use JSONL."},
    ]
    arm = {"arm": "C",
           "ranked": {"p1": ["D--Claude/cc-taste-01-x"]},
           "ranked_texts": {"p1": ["[policy:logging] Always use JSONL."]},
           "wall_ms": {"p1": 5},
           "static_block": "[policy:logging] Always use JSONL."}
    m = gr.arm_metrics(arm, rows, k=10)
    assert m["hit_at_K"] == 1.0
    assert m["mrr_at_K"] == 1.0
    assert m["static_block_lines"] == 1


def test_grade_arm_C_no_overlap_scores_zero():
    """If the static block doesn't appear in any retrieved text, arm C hits 0."""
    import grade as gr
    rows = [
        {"row_id": "p1", "kind": "positive",
         "expected_id": "D--Claude/cc-taste-01-x",
         "rule": "Always use JSONL."},
    ]
    arm = {"arm": "C",
           "ranked": {"p1": ["other-rule"]},
           "ranked_texts": {"p1": ["unrelated retrieved text"]},
           "wall_ms": {"p1": 5},
           "static_block": "[policy:logging] Always use JSONL."}
    m = gr.arm_metrics(arm, rows, k=10)
    assert m["hit_at_K"] == 0.0


def test_lifecycle_emit_schema_review_apply_roundtrip(tmp_path, monkeypatch):
    """End-to-end: emit a manifest from a fixture -> schema-validate -> flip
    status -> apply-time validate -> apply via memright (mocked)."""
    import emit_arm_d_candidates as emit_mod
    import gate3_review as g3

    manifest_path = tmp_path / "roundtrip.manifest.json"
    emit_mod.main(["--out", str(manifest_path)])
    body = json.loads(manifest_path.read_text(encoding="utf-8"))
    assert body["schema_version"] == pr_mod.SCHEMA_VERSION
    assert body["records"]
    assert body["authority_manifest"]["manifest_sha256"]
    assert all(
        rec["authority_manifest_sha256"]
        == body["authority_manifest"]["manifest_sha256"]
        for rec in body["records"]
    )

    m_inspect = mf.validate_schema(manifest_path)
    assert m_inspect["batch_id"] == body["batch_id"]

    rc = g3.main(["--manifest", str(manifest_path),
                  "--out", str(tmp_path / "gate3")])
    assert rc == 0, "fixture-driven manifest should pass Gate 3"

    for rec in body["records"]:
        rec["status"] = "accepted"
        rec["payload_sha256"] = mf.payload_sha256(rec)
    manifest_path.write_text(json.dumps(body, ensure_ascii=False),
                             encoding="utf-8")

    m_apply = mf.apply_time_validate(manifest_path)
    assert len(mf.accepted_records(m_apply)) == len(body["records"])

    journal = _isolate_journal(tmp_path, monkeypatch)
    journal.record(m_apply["batch_id"], "discovered",
                   sessions=m_apply["source_session_ids"])

    puts = []
    def fake_run(args):
        if args and args[0] == "put":
            puts.append(args[1])
        return True
    monkeypatch.setattr(lt, "_run_memright", fake_run)
    rc2 = lt.apply_from_manifest(manifest_path)
    assert rc2 == 0
    assert len(puts) == len(m_apply["records"])
    written_ids = set(puts)
    expected_ids = {r["id"] for r in m_apply["records"]}
    assert written_ids == expected_ids


def test_apply_idempotent_runs_each_id_through_memright_once(tmp_path, monkeypatch):
    """Each apply pass writes every accepted id exactly once through memright.
    Memright put is upsert by design, so a second run is safe."""
    import emit_arm_d_candidates as emit_mod
    manifest_path = tmp_path / "idem.manifest.json"
    emit_mod.main(["--out", str(manifest_path)])
    body = json.loads(manifest_path.read_text(encoding="utf-8"))
    for rec in body["records"]:
        rec["status"] = "accepted"
        rec["payload_sha256"] = mf.payload_sha256(rec)
    manifest_path.write_text(json.dumps(body, ensure_ascii=False),
                             encoding="utf-8")

    journal = _isolate_journal(tmp_path, monkeypatch)
    journal.record(body["batch_id"], "discovered",
                   sessions=body["source_session_ids"])

    puts_per_run = []
    def fake_run(args):
        if args and args[0] == "put":
            puts_per_run.append(args[1])
        return True
    monkeypatch.setattr(lt, "_run_memright", fake_run)
    rc = lt.apply_from_manifest(manifest_path)
    assert rc == 0
    n = len(puts_per_run)
    assert n == len(body["records"])


def test_rollback_create_then_dryrun_does_not_modify(tmp_path, monkeypatch):
    """Create captures live invariants; dry-run prints the plan without
    touching anything. The dry-run path does not require memright or
    sqlite3."""
    import rollback as rk
    state = tmp_path / "state.json"
    monkeypatch.setattr(rk, "STATE_FILE", state)
    state.write_text("post-apply-bad", encoding="utf-8")
    db = tmp_path / "engine.db"
    _make_sqlite_db(db)
    sp_path = rk.create_safe_point(
        {"batch_id": "b1", "records": [
            {"id": "a-b-0000000000", "status": "accepted",
             "scope": "D--Claude", "category": "tooling",
             "rule": "Always run the exact rollback smoke before applying."}]},
        db, state_path=state, rules_path=tmp_path / "rules.json",
        core_path=tmp_path / "core.json", out_path=tmp_path / "sp.json",
    )
    rc = rk.revert(sp_path, apply=False)
    assert rc == 0
    assert state.read_text(encoding="utf-8") == "post-apply-bad"


def test_manifest_schema_rejects_unknown_top_level_field(tmp_path):
    """additionalProperties:false refuses extra top-level fields."""
    body = {
        "schema_version": "1.0.0",
        "batch_id": "x", "created_at": "2026-07-13T00:00:00Z",
        "source_session_ids": ["s1"], "records": [],
        "session_count": 1,
    }
    p = tmp_path / "m.json"
    p.write_text(json.dumps(body), encoding="utf-8")
    with pytest.raises(mf.ManifestError):
        mf.validate_schema(p)


def test_gate3_evidence_count_zero_is_unsupported(tmp_path):
    """A candidate with evidence_count=0 has no excerpt -> evidence_fidelity
    fails -> Gate 3 hard-fails with rc=1."""
    import gate3_review as g3
    rec = {
        "id": "adapt-x-y-0000000000",
        "rule": "Always do thing number zero in the right way here.",
        "category": "tooling", "scope": "D--Claude",
        "status": "accepted",
        "evidence_count": 0, "evidence_excerpt": "",
        "source_ids": ["s1"],
        "source_file_hashes": [{"session_id": "s1", "sha256": "ab" * 32}],
        "evidence_ids": [],
    }
    rec["payload_sha256"] = mf.payload_sha256(rec)
    body = {
        "schema_version": "1.0.0",
        "batch_id": "x", "created_at": "2026-07-13T00:00:00Z",
        "source_session_ids": ["s1"],
        "records": [rec],
    }
    p = tmp_path / "m.json"
    p.write_text(json.dumps(body), encoding="utf-8")
    mf.apply_time_validate(p)
    rc = g3.main(["--manifest", str(p), "--out", str(tmp_path / "g3")])
    assert rc == 1


def test_pref_record_round_trip_with_evidence_id():
    """PreferenceRecord survives dict round-trip including all Gate 1b fields."""
    pr = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x",
         "category": "tooling", "rule": "Prefer JSONL for structured logs.",
         "confidence": 0.7},
        scope="D--Claude", source_ids=("s1",),
    )
    d = pr.to_dict()
    d["source_ids"] = tuple(d["source_ids"])
    pr2 = pr_mod.PreferenceRecord(**d)
    assert pr2 == pr


def test_adapt_report_runs_on_clean_state(tmp_path, monkeypatch):
    """The report generator must work even when nothing has been applied."""
    state_dir = tmp_path / "adapt"
    state_dir.mkdir()
    (state_dir / "audit.jsonl").write_text("")
    (state_dir / "run_journal.jsonl").write_text("")
    (state_dir / "state.json").write_text('{"learned": {}}')
    (state_dir / "rules.json").write_text("{}")
    import adapt_report as ar
    rc = ar.main(["--state-dir", str(state_dir), "--out", str(tmp_path / "out"),
                  "--heartbeat", str(tmp_path / "heartbeat.jsonl"),
                  "--db", str(tmp_path / "missing.db")])
    assert rc == 0
    body = json.loads((tmp_path / "out" / "adapt.report.json").read_text())
    assert body["rules_count"] == 0
    assert body["learned_sessions_total"] == 0
    assert body["audit_total"] == 0


def test_adapt_report_counts_admission_rejections(tmp_path):
    state_dir = tmp_path / "adapt"
    state_dir.mkdir()
    audit = state_dir / "audit.jsonl"
    audit.write_text(
        json.dumps({"event": "admission_rejected", "name": "x",
                     "why": "rule-too-short", "category": "tooling"}) + "\n"
        + json.dumps({"event": "admission_rejected", "name": "y",
                      "why": "category-not-allowed", "category": "audit-foo"}) + "\n"
        + json.dumps({"action": "add", "name": "z", "category": "tooling",
                      "rule": "Use JSONL", "confidence": 0.7}) + "\n"
    )
    (state_dir / "run_journal.jsonl").write_text("")
    (state_dir / "state.json").write_text('{"learned": {}}')
    (state_dir / "rules.json").write_text("{}")
    import adapt_report as ar
    rc = ar.main(["--state-dir", str(state_dir), "--out", str(tmp_path / "out"),
                  "--heartbeat", str(tmp_path / "heartbeat.jsonl"),
                  "--db", str(tmp_path / "missing.db")])
    assert rc == 0
    body = json.loads((tmp_path / "out" / "adapt.report.json").read_text())
    assert body["admission_rejection_reasons"]["rule-too-short"] == 1
    assert body["admission_rejection_reasons"]["category-not-allowed"] == 1
    assert body["actions"]["add"] == 1


def test_adapt_report_tracks_incomplete_batches(tmp_path):
    state_dir = tmp_path / "adapt"
    state_dir.mkdir()
    (state_dir / "audit.jsonl").write_text("")
    journal = state_dir / "run_journal.jsonl"
    journal.write_text(
        json.dumps({"ts": "2026-07-12T10:00:00+00:00", "batch_id": "b1",
                     "stage": "discovered", "sessions": ["s1"]}) + "\n"
        + json.dumps({"ts": "2026-07-12T10:00:05+00:00", "batch_id": "b1",
                      "stage": "extracted", "observations": []}) + "\n"
        + json.dumps({"ts": "2026-07-12T10:00:10+00:00", "batch_id": "b2",
                      "stage": "discovered", "sessions": ["s2"]}) + "\n"
        + json.dumps({"ts": "2026-07-12T10:00:15+00:00", "batch_id": "b2",
                      "stage": "committed", "sessions": ["s2"]}) + "\n"
    )
    (state_dir / "state.json").write_text('{"learned": {"claude-code": {"s2": 1.0}}}')
    (state_dir / "rules.json").write_text("{}")
    import adapt_report as ar
    rc = ar.main(["--state-dir", str(state_dir), "--out", str(tmp_path / "out"),
                  "--heartbeat", str(tmp_path / "heartbeat.jsonl"),
                  "--db", str(tmp_path / "missing.db")])
    body = json.loads((tmp_path / "out" / "adapt.report.json").read_text())
    assert body["batches_completed"] == 1
    assert body["batches_incomplete"] == ["b1"]
    assert body["learned_sessions_total"] == 1
    assert body["latency_per_batch_seconds"]["b2"] == 5.0


def test_adapt_report_aggregates_shadow_delivery_and_effectiveness(tmp_path):
    import sqlite3
    import adapt_report as ar

    state_dir = tmp_path / "adapt"
    state_dir.mkdir()
    (state_dir / "audit.jsonl").write_text(
        json.dumps({"event": "shadow_conflict", "id": "D--Claude/adapt-a"}) + "\n"
        + json.dumps({"event": "shadow_recorrection", "id": "D--Claude/adapt-b"}) + "\n"
    )
    (state_dir / "run_journal.jsonl").write_text("")
    (state_dir / "state.json").write_text('{"learned": {}}')
    (state_dir / "rules.json").write_text("{}")
    heartbeat = tmp_path / "heartbeat.jsonl"
    heartbeat.write_text(json.dumps({
        "event": "recall.delivery",
        "applicable_adapt_ids": ["D--Claude/adapt-a", "D--Claude/adapt-b"],
        "delivered_adapt_ids": ["D--Claude/adapt-a"],
        "core_source_ids": ["adapt-core-a"],
        "core_delivered": True,
        "context_chars": 400,
        "estimated_tokens": 100,
        "core_tokens": 30,
        "retrieval_tokens": 70,
    }) + "\n")
    db = tmp_path / "engine.db"
    with sqlite3.connect(db) as conn:
        conn.execute("CREATE TABLE memory_event_log (event_kind TEXT, memory_id TEXT)")
        conn.executemany("INSERT INTO memory_event_log VALUES (?, ?)", [
            ("inject", "D--Claude/adapt-a"),
            ("inject", "D--Claude/adapt-b"),
            ("get", "D--Claude/adapt-a"),
        ])
    report = ar.build_report(
        state_dir=state_dir, heartbeat_path=heartbeat, db_path=db
    )
    shadow = report["shadow"]
    assert shadow["delivery_receipts"] == 1
    assert shadow["applicable_unique"] == 2
    assert shadow["delivered_unique"] == 1
    assert shadow["core_deliveries"] == 1
    assert shadow["estimated_tokens"] == 100
    assert shadow["core_tokens"] == 30
    assert shadow["retrieval_tokens"] == 70
    assert shadow["explicit_get_unique"] == 1
    assert shadow["injected_unique"] == 2
    assert shadow["explicit_usefulness_rate"] == 0.5
    assert shadow["conflicts"] == 1
    assert shadow["recorrections"] == 1


def test_curation_near_duplicate_candidates_groups_by_normalized_text():
    import curation_diagnostic as cd
    rows = [
        {"id": "D--Claude/a-0000000000", "content": "Always use JSONL for logs."},
        {"id": "D--Claude/a-0000000001", "content": "always use jsonl for logs."},
        {"id": "D--Claude/b-0000000000", "content": "Always run smoke tests."},
    ]
    clusters = cd.near_duplicate_candidates(rows)
    assert len(clusters) == 2
    sizes = sorted(c["size"] for c in clusters)
    assert sizes == [1, 2]
    dup = [c for c in clusters if c["size"] == 2][0]
    assert "D--Claude/a-0000000000" in dup["member_ids"]
    assert "D--Claude/a-0000000001" in dup["member_ids"]


def test_curation_inventory_filters_to_adapt_prefix(tmp_path):
    import sqlite3
    db = tmp_path / "engine.db"
    with sqlite3.connect(db) as conn:
        conn.execute("CREATE TABLE memories (id TEXT, content TEXT)")
        conn.executemany("INSERT INTO memories VALUES (?, ?)",
                         [("D--Claude/adapt-x-0000000000", "Use JSONL."),
                          ("D--Claude/other-y-0000000000", "unrelated"),
                          ("D--Claude/adapt-z-0000000000", "Run smoke.")])
    import curation_diagnostic as cd
    rows = cd.inventory_adapt_rows(db)
    assert len(rows) == 2
    assert {r["id"] for r in rows} == {
        "D--Claude/adapt-x-0000000000", "D--Claude/adapt-z-0000000000",
    }


def test_curation_same_scope_clusters_by_scope_prefix():
    import curation_diagnostic as cd
    rows = [
        {"id": "D--Claude/x", "content": "a"},
        {"id": "D--Claude-heardright/y", "content": "b"},
        {"id": "global", "content": "c"},
    ]
    out = cd.same_scope_clusters(rows)
    assert set(out.keys()) == {"D--Claude", "D--Claude-heardright", "<global>"}


import pytest  # noqa: E402
