"""Offline tests for the adapt pipeline. No network, no memright binary."""
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

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


def test_scanner_positive_drops_turn_without_raw_secret(tmp_path, monkeypatch):
    monkeypatch.setattr(ts, "scanner_clean", lambda text: False)
    rows = [{"type": "user", "userType": "external", "cwd": "D:\\Claude", "sessionId": "s",
             "message": {"content": "always use the special token [REDACTED] carefully"}}]
    assert ts.parse_claude_session(_write(tmp_path / "s.jsonl", rows)) is None


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


def test_batching_respects_char_budget():
    turns = [("claude-code", "D--Claude", f"turn number {i} " + "y" * 300) for i in range(400)]
    batches = tl.build_batches(turns, budget=5_000)
    assert all(sum(len(t[2]) for t in b) <= 5_000 for b in batches)
    assert sum(len(b) for b in batches) == 400


def test_extract_parses_model_json():
    def fake(system, user):
        assert json.loads(user)[0]["text"] == "always use JSONL"
        return json.dumps([
            {"category": "logging", "observation": "use JSONL not logfmt",
             "evidence": "always use JSONL", "prompt": 1}])
    obs = tl.extract_observations([("claude-code", "D--Claude", "always use JSONL")], llm=fake)
    # Deterministic extractor runs first; LLM result is appended.
    categories = sorted(o["category"] for o in obs)
    assert categories == ["explicit-preference", "logging"]
    llm_obs = next(o for o in obs if o["category"] == "logging")
    assert llm_obs["observation"] == "use JSONL not logfmt"
    assert llm_obs["tool"] == "claude-code" and llm_obs["scope"] == "D--Claude"


def test_deterministic_extract_catches_explicit_preferences():
    obs = tl.extract_deterministic([("claude-code", "D--Claude",
                                     "always use JSONL for structured logging, not logfmt")])
    assert obs and obs[0]["category"] == "explicit-preference"
    assert "always use JSONL" in obs[0]["evidence"]


def test_extract_tolerates_fenced_and_audits_junk_output(tmp_path, monkeypatch):
    monkeypatch.setattr(tl.Path, "home", lambda: tmp_path)
    fake = lambda system, user: "```json\n[{\"category\":\"x\",\"observation\":\"y\",\"evidence\":\"z\",\"prompt\":1}]\n```"
    assert len(tl.extract_observations([("codex", "D--Claude", "hello world turn")], llm=fake)) == 1
    fake_bad = lambda system, user: "no json here"
    assert tl.extract_observations([("codex", "D--Claude", "hello world turn")], llm=fake_bad) == []
    audit = tmp_path / ".claude" / "adapt" / "audit.jsonl"
    assert "parse_failure" in audit.read_text(encoding="utf-8")


def test_llm_call_failure_is_audited_and_raised(tmp_path, monkeypatch):
    monkeypatch.setattr(tl.Path, "home", lambda: tmp_path)
    def boom(system, user):
        raise TimeoutError("network")
    try:
        tl.extract_observations([("codex", "D--Claude", "hello world turn")], llm=boom)
    except TimeoutError:
        pass
    else:
        raise AssertionError("expected TimeoutError")
    audit = tmp_path / ".claude" / "adapt" / "audit.jsonl"
    assert "llm_call_failed" in audit.read_text(encoding="utf-8")


def test_synthesize_merges_against_existing():
    existing = [{"name": "adapt-logging-jsonl-over-logfmt", "category": "logging",
                 "rule": "Use JSONL for structured logging.", "confidence": 0.8,
                 "observations": 2, "scope": "D--Claude"}]
    new_obs = [{"category": "logging", "observation": "use JSONL not logfmt",
                "evidence": "always use JSONL", "tool": "codex", "scope": "D--Claude"}]
    fake = lambda system, user: json.dumps([
        {"action": "update", "name": "adapt-logging-jsonl-over-logfmt", "category": "logging",
         "rule": "Use JSONL for structured logging, never logfmt.", "confidence": 0.85,
         "observations": 3, "why": "re-endorsed in codex session"}])
    actions = tl.synthesize("logging", existing, new_obs, llm=fake)
    assert actions[0]["action"] == "update"
    assert actions[0]["confidence"] == 0.85


def test_synthesize_preserves_low_confidence_as_review_needed():
    fake = lambda system, user: json.dumps([
        {"action": "add", "name": "adapt-review-ask-first", "category": "review",
         "rule": "Ask before broad review changes.", "confidence": 0.35,
         "observations": 1, "why": "single weak hint"}])
    actions = tl.synthesize("review", [], [{"category": "review", "observation": "ask first"}], llm=fake)
    assert actions[0]["confidence"] == 0.35
    assert actions[0]["needs_review"] is True


# --- Task 3: orchestrator tests ---
import importlib.util

_spec = importlib.util.spec_from_file_location(
    "adapt_cli", Path(__file__).resolve().parent / "adapt.py")
lt = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(lt)


def test_apply_actions_upserts_via_memright(tmp_path, monkeypatch):
    calls = []
    monkeypatch.setattr(lt, "_run_memright", lambda args: calls.append(args) or True)
    rules = {}
    actions = [{"action": "add", "name": "adapt-logging-jsonl-over-logfmt",
                "category": "logging", "rule": "Use JSONL for structured logging.",
                "confidence": 0.7, "observations": 1, "why": "stated directly"}]
    obs_by_cat = {"logging": [{"scope": "D--Claude", "tool": "claude-code",
                               "evidence": "always use JSONL", "category": "logging",
                               "observation": "use JSONL"}]}
    changed, ok = lt.apply_actions(actions, obs_by_cat, rules, tmp_path, dry_run=False)
    assert changed == 1 and ok is True
    assert "adapt-logging-jsonl-over-logfmt" in rules
    assert len(calls) == 1
    assert calls[0][:2] == ["put", "adapt-logging-jsonl-over-logfmt"]
    assert "--scope" in calls[0] and "D--Claude" in calls[0]


def test_apply_is_idempotent_on_keep(tmp_path, monkeypatch):
    calls = []
    monkeypatch.setattr(lt, "_run_memright", lambda args: calls.append(args) or True)
    rules = {"adapt-logging-jsonl-over-logfmt": {
        "name": "adapt-logging-jsonl-over-logfmt", "category": "logging",
        "rule": "Use JSONL.", "confidence": 0.7, "observations": 1, "scope": "D--Claude"}}
    actions = [{"action": "keep", "name": "adapt-logging-jsonl-over-logfmt",
                "category": "logging", "rule": "Use JSONL.", "confidence": 0.7,
                "observations": 1, "why": ""}]
    changed, ok = lt.apply_actions(actions, {"logging": []}, rules, tmp_path, dry_run=False)
    assert changed == 0 and ok is True
    assert calls == []          # keep = no write


def test_dry_run_never_writes(tmp_path, monkeypatch):
    calls = []
    monkeypatch.setattr(lt, "_run_memright", lambda args: calls.append(args) or True)
    actions = [{"action": "add", "name": "adapt-x-y", "category": "x", "rule": "r",
                "confidence": 0.6, "observations": 1, "why": ""}]
    rules = {}
    changed, ok = lt.apply_actions(actions, {"x": []}, rules, tmp_path, dry_run=True)
    assert changed == 0 and ok is True
    assert calls == [] and rules == {}


def test_failed_memright_write_reports_not_ok(tmp_path, monkeypatch):
    monkeypatch.setattr(lt, "_run_memright", lambda args: False)
    actions = [{"action": "add", "name": "adapt-x-y", "category": "x", "rule": "r",
                "confidence": 0.6, "observations": 1, "why": ""}]
    changed, ok = lt.apply_actions(actions, {"x": []}, {}, tmp_path, dry_run=False)
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
