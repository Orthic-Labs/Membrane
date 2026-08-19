"""Token-spend analysis: exact totals, bounded attribution, honest findings."""
from __future__ import annotations

import json
from pathlib import Path

import pytest

from adapt import token_spend


def _claude_row(row_type: str, content, **extra) -> dict:
    message = {"content": content}
    message.update(extra.pop("message", {}))
    return {"type": row_type, "timestamp": "2026-08-19T00:00:00Z",
            "sessionId": "s1", "message": message, **extra}


def _write(tmp_path: Path, rows: list[dict]) -> Path:
    path = tmp_path / "transcript.jsonl"
    path.write_text("\n".join(json.dumps(row) for row in rows), encoding="utf-8")
    return path


def _assistant(msg_id: str, usage: dict, blocks: list[dict], model: str = "claude-opus-5") -> dict:
    return _claude_row("assistant", blocks,
                       message={"id": msg_id, "model": model, "usage": usage})


def _usage(inp=0, read=0, create=0, out=0, thinking=0) -> dict:
    return {"input_tokens": inp, "cache_read_input_tokens": read,
            "cache_creation_input_tokens": create, "output_tokens": out,
            "output_tokens_details": {"thinking_tokens": thinking}}


def test_totals_are_exact_provider_counts(tmp_path):
    path = _write(tmp_path, [
        _claude_row("user", "hello"),
        _assistant("m1", _usage(inp=100, create=900, out=50, thinking=10),
                   [{"type": "text", "text": "hi"}]),
    ])
    report = token_spend.analyze(path)
    totals = report["totals"]
    assert totals["requests"] == 1
    assert totals["inputTokens"] == 100
    assert totals["cacheCreateTokens"] == 900
    assert totals["outputTokens"] == 50
    assert totals["thinkingTokens"] == 10
    assert totals["billedTokens"] == 1050


def test_streamed_repeats_of_one_message_are_not_double_counted(tmp_path):
    usage = _usage(inp=10, read=1000, out=20)
    path = _write(tmp_path, [
        _claude_row("user", "hello"),
        _assistant("m1", usage, [{"type": "text", "text": "part one"}]),
        _assistant("m1", usage, [{"type": "text", "text": "part two"}]),
    ])
    report = token_spend.analyze(path)
    assert report["totals"]["requests"] == 1
    assert report["totals"]["cacheReadTokens"] == 1000


def test_growth_is_attributed_to_the_tool_result_that_caused_it(tmp_path):
    big = "x" * 40_000
    path = _write(tmp_path, [
        _claude_row("user", "run it"),
        _assistant("m1", _usage(inp=1000, out=10),
                   [{"type": "tool_use", "id": "c1", "name": "Bash", "input": {"command": "ls"}}]),
        _claude_row("user", [{"type": "tool_result", "tool_use_id": "c1", "content": big}]),
        _assistant("m2", _usage(inp=12_000, out=10), [{"type": "text", "text": "done"}]),
    ])
    report = token_spend.analyze(path, include_turns=True)
    tools = {entry["tool"]: entry["attributedTokens"] for entry in report["attributedByTool"]}
    assert tools["Bash"] > 9_000
    # Growth measured, not invented: 12000 - 1000 = 11000, minus the echoed output.
    assert report["turns"][1]["growthTokens"] == 11_000


def test_attribution_cannot_exceed_what_the_text_could_cost(tmp_path):
    """A tiny message must not absorb a huge system-prefix load."""
    path = _write(tmp_path, [
        _claude_row("user", "hi"),
        _assistant("m1", _usage(inp=50_000, out=10), [{"type": "text", "text": "ok"}]),
    ])
    report = token_spend.analyze(path)
    kinds = report["attributedByKind"]
    assert kinds.get("user_message", 0) < 100
    assert kinds["session_prefix"] > 49_000


def test_oversized_tool_result_is_flagged_with_its_cost(tmp_path):
    path = _write(tmp_path, [
        _claude_row("user", "read the log"),
        _assistant("m1", _usage(inp=1_000, out=10),
                   [{"type": "tool_use", "id": "c1", "name": "Read", "input": {"file": "big.log"}}]),
        _claude_row("user", [{"type": "tool_result", "tool_use_id": "c1", "content": "y" * 200_000}]),
        _assistant("m2", _usage(inp=41_000, out=10), [{"type": "text", "text": "ok"}]),
    ])
    report = token_spend.analyze(path)
    slugs = {finding["detector"] for finding in report["findings"]}
    assert "oversized_tool_result" in slugs
    finding = next(f for f in report["findings"] if f["detector"] == "oversized_tool_result")
    assert finding["wastedTokens"] >= token_spend.LARGE_CONTRIBUTION_TOKENS
    assert finding["likelyMechanism"].startswith("candidate:")


def test_duplicate_tool_calls_are_costed_once_as_waste(tmp_path):
    payload = "z" * 60_000
    rows = [_claude_row("user", "go")]
    context = 1_000
    for index in range(2):
        rows.append(_assistant(f"m{index}a", _usage(inp=context, out=10),
                               [{"type": "tool_use", "id": f"c{index}", "name": "Bash",
                                 "input": {"command": "pytest -q"}}]))
        context += 20_000
        rows.append(_claude_row("user", [{"type": "tool_result", "tool_use_id": f"c{index}",
                                          "content": payload}]))
    rows.append(_assistant("mz", _usage(inp=context, out=10), [{"type": "text", "text": "done"}]))
    report = token_spend.analyze(_write(tmp_path, rows))
    finding = next(f for f in report["findings"] if f["detector"] == "duplicate_tool_call_cost")
    assert finding["wastedTokens"] > 0


def test_empty_tool_input_does_not_look_like_a_duplicate(tmp_path):
    rows = [_claude_row("user", "go")]
    context = 1_000
    for index in range(3):
        rows.append(_assistant(f"m{index}", _usage(inp=context, out=10),
                               [{"type": "tool_use", "id": f"c{index}", "name": "Status", "input": {}}]))
        context += 15_000
        rows.append(_claude_row("user", [{"type": "tool_result", "tool_use_id": f"c{index}",
                                          "content": "w" * 50_000}]))
    rows.append(_assistant("mz", _usage(inp=context, out=10), [{"type": "text", "text": "done"}]))
    report = token_spend.analyze(_write(tmp_path, rows))
    assert "duplicate_tool_call_cost" not in {f["detector"] for f in report["findings"]}


def test_subagent_lane_is_tracked_separately(tmp_path):
    path = _write(tmp_path, [
        _claude_row("user", "go"),
        _assistant("m1", _usage(inp=1_000, out=10), [{"type": "text", "text": "ok"}]),
        _claude_row("assistant", [{"type": "text", "text": "sub"}], isSidechain=True,
                    message={"id": "m2", "model": "claude-haiku-4-5-20251001",
                             "usage": _usage(inp=500, out=5)}),
    ])
    report = token_spend.analyze(path)
    assert set(report["byLane"]) == {"main", "subagent"}
    assert report["byLane"]["subagent"]["billedTokens"] == 505


def test_codex_token_count_rows_are_read(tmp_path):
    path = tmp_path / "rollout.jsonl"
    rows = [
        {"type": "session_meta", "payload": {"id": "cx1"}},
        {"type": "response_item", "payload": {"type": "function_call", "call_id": "c1",
                                              "name": "exec", "arguments": "ls"}},
        {"type": "response_item", "payload": {"type": "function_call_output", "call_id": "c1",
                                              "output": "q" * 4_000}},
        {"type": "event_msg", "timestamp": "2026-08-19T00:00:00Z",
         "payload": {"type": "token_count", "info": {"last_token_usage": {
             "input_tokens": 5_000, "cached_input_tokens": 4_000,
             "cache_write_input_tokens": 0, "output_tokens": 100,
             "reasoning_output_tokens": 20}}}},
    ]
    path.write_text("\n".join(json.dumps(row) for row in rows), encoding="utf-8")
    report = token_spend.analyze(path)
    assert report["totals"]["requests"] == 1
    assert report["totals"]["cacheReadTokens"] == 4_000
    assert report["totals"]["inputTokens"] == 1_000


def test_no_cost_without_caller_supplied_rates(tmp_path):
    path = _write(tmp_path, [
        _claude_row("user", "hi"),
        _assistant("m1", _usage(inp=1_000_000, out=1_000_000), [{"type": "text", "text": "ok"}]),
    ])
    assert "estimatedCost" not in token_spend.analyze(path)
    priced = token_spend.analyze(path, rates={"claude-opus-5": {"input": 5.0, "output": 25.0}})
    assert priced["estimatedCost"]["total"] == pytest.approx(30.0)


def test_report_is_stable_and_serializable(tmp_path):
    path = _write(tmp_path, [
        _claude_row("user", "hi"),
        _assistant("m1", _usage(inp=1_000, out=10), [{"type": "text", "text": "ok"}]),
    ])
    first, second = token_spend.analyze(path), token_spend.analyze(path)
    assert json.dumps(first, sort_keys=True) == json.dumps(second, sort_keys=True)
    assert token_spend.HONESTY_LIMIT in token_spend.render_text(first)


def test_missing_or_garbage_transcript_does_not_raise(tmp_path):
    assert token_spend.analyze(tmp_path / "nope.jsonl")["totals"]["requests"] == 0
    junk = tmp_path / "junk.jsonl"
    junk.write_text("not json\n{}\n", encoding="utf-8")
    assert token_spend.analyze(junk)["totals"]["requests"] == 0


def test_insights_report_carries_the_spend_block(tmp_path):
    from adapt import insights

    path = _write(tmp_path, [
        _claude_row("user", "hi"),
        _assistant("m1", _usage(inp=1_000, out=10), [{"type": "text", "text": "ok"}]),
    ])
    report = insights.report_many([path])
    assert report["tokenSpend"]["totals"]["billedTokens"] == 1_010
    assert report["sessionSummaries"][0]["tokenSpend"]["totals"]["requests"] == 1
