"""Token-spend analysis for Adapt Insights (report-only).

Where did the tokens go, and which of them were wasted?

Scope and honesty limit
-----------------------
This module reads *exact provider-reported* usage already present in the
transcript — it does not tokenize, estimate, or call any API. Claude Code
records ``message.usage`` on every assistant row; Codex records
``token_count`` rows. Both are billed counts straight from the provider,
so the totals here are facts, not estimates.

Attribution, by contrast, is a *heuristic*: we know a turn's billed input
grew by N tokens, and we know which tool results and user text entered the
context since the previous turn, but the provider does not tell us how the
growth splits between them. We split proportionally by serialized length
and label every attributed number ``attributed`` so no consumer mistakes
it for a measurement. Totals are measured; per-tool splits are inferred.

Nothing here writes to Membrane, Taste, or any database. Same hard
constraint as :mod:`adapt.insights` (plan 5.5): report only.
"""

from __future__ import annotations

import hashlib
import json
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Iterable

HONESTY_LIMIT = (
    "Totals are provider-reported billed counts read from the transcript. "
    "Per-tool and per-turn attribution is heuristic: context growth is split "
    "proportionally across the tool results and messages that entered since "
    "the previous request. Fields named 'attributed*' are inferences."
)

SCHEMA_VERSION = "adapt.token-spend.v1"

# Thresholds — deliberately conservative so cards stay signal, not noise.
LARGE_CONTRIBUTION_TOKENS = 10_000     # one tool result this big is worth naming
HUGE_CONTRIBUTION_TOKENS = 25_000
COLD_CACHE_TOKENS = 20_000             # full context rebuilt with no cache read
DOMINANT_TOOL_SHARE = 0.35             # one tool owning this much of all growth
REPEAT_CALL_MIN = 2                    # identical call this many times

# Rough bytes-per-token used ONLY to bound attribution, never to report a
# token count. English prose and JSON both land near 3.5-4 chars/token; the
# tolerance keeps the bound from under-charging dense or non-Latin text.
CHARS_PER_TOKEN = 3.5
ATTRIBUTION_TOLERANCE = 1.6


# ---------------------------------------------------------------------------
# Turn records
# ---------------------------------------------------------------------------

@dataclass
class Turn:
    """One billed provider request, with what entered context before it."""

    rowIndex: int
    timestamp: str | None
    sessionId: str
    host: str
    model: str
    sidechain: bool
    requestId: str
    messageId: str
    inputTokens: int = 0
    cacheReadTokens: int = 0
    cacheCreateTokens: int = 0
    outputTokens: int = 0
    thinkingTokens: int = 0
    # Contributions that entered context since the previous request.
    contributions: list[dict[str, Any]] = field(default_factory=list)

    @property
    def contextTokens(self) -> int:
        """Total billed input for this request (fresh + cache read + cache write)."""
        return self.inputTokens + self.cacheReadTokens + self.cacheCreateTokens

    @property
    def billedTokens(self) -> int:
        return self.contextTokens + self.outputTokens


def _as_int(value: Any) -> int:
    return value if isinstance(value, int) and not isinstance(value, bool) else 0


def _rows(path: str | Path) -> Iterable[tuple[int, dict[str, Any]]]:
    try:
        text = Path(path).read_text(encoding="utf-8", errors="replace")
    except OSError:
        return
    for index, line in enumerate(text.splitlines()):
        line = line.strip()
        if not line:
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(obj, dict):
            yield index + 1, obj


def _contribution(kind: str, tool: str, label: str, chars: int) -> dict[str, Any]:
    return {"kind": kind, "tool": tool, "label": label, "chars": max(0, chars)}


def _input_digest(payload: Any) -> str:
    encoded = json.dumps(payload, sort_keys=True, ensure_ascii=False, default=str)
    return hashlib.sha256(encoded.encode("utf-8")).hexdigest()[:16]


# ---------------------------------------------------------------------------
# Claude Code
# ---------------------------------------------------------------------------

def _parse_claude(path: str | Path) -> list[Turn]:
    turns: list[Turn] = []
    seen_messages: dict[str, Turn] = {}
    pending: list[dict[str, Any]] = []
    call_tool: dict[str, str] = {}
    call_input: dict[str, Any] = {}

    for row_index, obj in _rows(path):
        row_type = obj.get("type")
        if row_type not in {"user", "assistant"}:
            continue
        message = obj.get("message")
        if not isinstance(message, dict):
            continue
        session_id = str(obj.get("sessionId") or "")
        sidechain = bool(obj.get("isSidechain"))
        content = message.get("content")
        blocks = content if isinstance(content, list) else []

        if row_type == "user":
            if isinstance(content, str) and content.strip():
                pending.append(_contribution("user_message", "", "user message", len(content)))
            for block in blocks:
                if not isinstance(block, dict):
                    continue
                if block.get("type") == "tool_result":
                    value = block.get("content", "")
                    if not isinstance(value, str):
                        value = json.dumps(value, ensure_ascii=False, default=str)
                    call_id = str(block.get("tool_use_id") or "")
                    tool = call_tool.get(call_id, "unknown")
                    entry = _contribution("tool_result", tool, tool or "tool result", len(value))
                    entry["callId"] = call_id
                    entry["isError"] = bool(block.get("is_error"))
                    if call_id in call_input:
                        entry["inputDigest"] = _input_digest(call_input[call_id])
                    pending.append(entry)
                elif block.get("type") == "text" and isinstance(block.get("text"), str):
                    pending.append(_contribution("user_message", "", "user message", len(block["text"])))
            continue

        # assistant row
        for block in blocks:
            if isinstance(block, dict) and block.get("type") == "tool_use":
                call_id = str(block.get("id") or "")
                if call_id:
                    call_tool[call_id] = str(block.get("name") or "unknown")
                    tool_args = block.get("input")
                    if tool_args not in (None, "", {}):
                        call_input[call_id] = tool_args

        usage = message.get("usage")
        if not isinstance(usage, dict):
            continue
        message_id = str(message.get("id") or f"row-{row_index}")
        details = usage.get("output_tokens_details")
        thinking = _as_int(details.get("thinking_tokens")) if isinstance(details, dict) else 0
        turn = Turn(
            rowIndex=row_index,
            timestamp=obj.get("timestamp"),
            sessionId=session_id,
            host="claude_code",
            model=str(message.get("model") or "unknown"),
            sidechain=sidechain,
            requestId=str(obj.get("requestId") or ""),
            messageId=message_id,
            inputTokens=_as_int(usage.get("input_tokens")),
            cacheReadTokens=_as_int(usage.get("cache_read_input_tokens")),
            cacheCreateTokens=_as_int(usage.get("cache_creation_input_tokens")),
            outputTokens=_as_int(usage.get("output_tokens")),
            thinkingTokens=thinking,
        )
        previous = seen_messages.get(message_id)
        if previous is not None:
            # Streamed continuation of the same assistant message: the
            # provider repeats usage. Keep the largest, do not double count.
            if turn.billedTokens > previous.billedTokens:
                previous.inputTokens = turn.inputTokens
                previous.cacheReadTokens = turn.cacheReadTokens
                previous.cacheCreateTokens = turn.cacheCreateTokens
                previous.outputTokens = turn.outputTokens
                previous.thinkingTokens = turn.thinkingTokens
            continue
        turn.contributions = pending
        pending = []
        seen_messages[message_id] = turn
        turns.append(turn)
    return turns


# ---------------------------------------------------------------------------
# Codex
# ---------------------------------------------------------------------------

def _parse_codex(path: str | Path) -> list[Turn]:
    turns: list[Turn] = []
    pending: list[dict[str, Any]] = []
    call_tool: dict[str, str] = {}
    call_input: dict[str, Any] = {}
    session_id = ""
    model = "unknown"

    for row_index, obj in _rows(path):
        payload = obj.get("payload") if isinstance(obj.get("payload"), dict) else {}
        if obj.get("type") == "session_meta":
            session_id = str(payload.get("id") or payload.get("session_id") or session_id)
            model = str(payload.get("model") or model)
            continue
        payload_type = payload.get("type")
        if payload_type in {"function_call", "custom_tool_call"}:
            call_id = str(payload.get("call_id") or "")
            if call_id:
                call_tool[call_id] = str(payload.get("name") or "unknown")
                arguments = payload.get("arguments")
                if arguments in (None, "", {}):
                    arguments = payload.get("input")
                # Only fingerprint calls whose arguments we actually captured;
                # an empty payload would make every call look identical.
                if arguments not in (None, "", {}):
                    call_input[call_id] = arguments
            continue
        if payload_type in {"function_call_output", "custom_tool_call_output"}:
            call_id = str(payload.get("call_id") or "")
            output = payload.get("output", "")
            if not isinstance(output, str):
                output = json.dumps(output, ensure_ascii=False, default=str)
            tool = call_tool.get(call_id, "unknown")
            entry = _contribution("tool_result", tool, tool, len(output))
            entry["callId"] = call_id
            if call_id in call_input:
                entry["inputDigest"] = _input_digest(call_input[call_id])
            pending.append(entry)
            continue
        if payload_type == "message" and payload.get("role") == "user":
            texts = [b.get("text", "") for b in payload.get("content", [])
                     if isinstance(b, dict) and isinstance(b.get("text"), str)]
            joined = "".join(texts)
            if joined.strip():
                pending.append(_contribution("user_message", "", "user message", len(joined)))
            continue
        if obj.get("type") != "token_count" and payload_type != "token_count":
            continue
        source = payload if payload_type == "token_count" else obj
        info = source.get("info") if isinstance(source.get("info"), dict) else {}
        last = info.get("last_token_usage") if isinstance(info.get("last_token_usage"), dict) else {}
        if not last:
            continue
        total_input = _as_int(last.get("input_tokens"))
        cached = _as_int(last.get("cached_input_tokens"))
        turn = Turn(
            rowIndex=row_index,
            timestamp=obj.get("timestamp"),
            sessionId=session_id,
            host="codex",
            model=str(info.get("model") or model),
            sidechain=False,
            requestId="",
            messageId=f"row-{row_index}",
            inputTokens=max(0, total_input - cached),
            cacheReadTokens=cached,
            cacheCreateTokens=_as_int(last.get("cache_write_input_tokens")),
            outputTokens=_as_int(last.get("output_tokens")),
            thinkingTokens=_as_int(last.get("reasoning_output_tokens")),
        )
        turn.contributions = pending
        pending = []
        turns.append(turn)
    return turns


def parse_turns(path: str | Path) -> list[Turn]:
    """Read billed-usage turns from a Claude Code or Codex JSONL transcript."""
    turns = _parse_claude(path)
    if turns:
        return turns
    return _parse_codex(path)


# ---------------------------------------------------------------------------
# Attribution
# ---------------------------------------------------------------------------

def attribute(turns: list[Turn]) -> list[dict[str, Any]]:
    """Split each turn's context growth across what entered since the last one.

    Heuristic, by construction. The measured quantity is ``growth`` — the
    increase in billed input between consecutive requests on the same
    thread. The assistant's own previous output is a known component of
    that growth (it is fed back verbatim), so it is subtracted first; the
    remainder is split across pending contributions by serialized length.
    """
    records: list[dict[str, Any]] = []
    previous_context: dict[bool, int] = {}
    previous_output: dict[bool, int] = {}
    for turn in turns:
        lane = turn.sidechain
        prior = previous_context.get(lane)
        growth = turn.contextTokens if prior is None else max(0, turn.contextTokens - prior)
        assistant_echo = min(growth, previous_output.get(lane, 0)) if prior is not None else 0
        remainder = max(0, growth - assistant_echo)
        total_chars = sum(item["chars"] for item in turn.contributions)
        attributed: list[dict[str, Any]] = []
        if assistant_echo:
            attributed.append({"kind": "assistant_output", "tool": "", "label": "assistant output",
                               "chars": 0, "attributedTokens": assistant_echo})
        # A contribution cannot plausibly cost more than its own text does.
        # Whatever growth exceeds that ceiling is prefix the transcript does
        # not contain: system prompt, tool definitions, memory, skills,
        # reminders. Naming it honestly beats charging it to the last message.
        ceiling = int(total_chars / CHARS_PER_TOKEN * ATTRIBUTION_TOLERANCE)
        divisible = min(remainder, ceiling) if total_chars else 0
        prefix = remainder - divisible
        if total_chars > 0 and divisible > 0:
            running = 0
            for position, item in enumerate(turn.contributions):
                if position == len(turn.contributions) - 1:
                    share = divisible - running
                else:
                    share = int(divisible * item["chars"] / total_chars)
                    running += share
                attributed.append({**item, "attributedTokens": max(0, share)})
        if prefix > 0:
            attributed.append({
                "kind": "session_prefix" if prior is None else "context_overhead",
                "tool": "", "chars": 0, "attributedTokens": prefix,
                "label": "system prompt, tool definitions, memory and injected context"
                         if prior is None else "context re-injected outside the transcript"})
        records.append({
            "rowIndex": turn.rowIndex, "timestamp": turn.timestamp,
            "sessionId": turn.sessionId, "host": turn.host, "model": turn.model,
            "sidechain": turn.sidechain,
            "contextTokens": turn.contextTokens, "inputTokens": turn.inputTokens,
            "cacheReadTokens": turn.cacheReadTokens, "cacheCreateTokens": turn.cacheCreateTokens,
            "outputTokens": turn.outputTokens, "thinkingTokens": turn.thinkingTokens,
            "growthTokens": growth, "attributed": attributed,
        })
        previous_context[lane] = turn.contextTokens
        previous_output[lane] = turn.outputTokens
    return records


# ---------------------------------------------------------------------------
# Cost (opt-in only — no rates are shipped, nothing is invented)
# ---------------------------------------------------------------------------

def apply_rates(totals: dict[str, Any], rates: dict[str, dict[str, float]] | None) -> dict[str, Any] | None:
    """Price a totals block against caller-supplied per-million-token rates.

    ``rates`` maps model id -> {"input", "output", "cacheRead", "cacheWrite"}
    in currency per million tokens. No rate table is bundled: prices change
    and a stale hard-coded number is a fabricated statistic, not a feature.
    """
    if not rates:
        return None
    priced: dict[str, Any] = {"currencyPerMillionTokens": True, "byModel": {}, "total": 0.0}
    for model, block in totals.get("byModel", {}).items():
        rate = rates.get(model) or rates.get("default")
        if not rate:
            continue
        cost = (
            block["inputTokens"] * rate.get("input", 0.0)
            + block["cacheReadTokens"] * rate.get("cacheRead", rate.get("input", 0.0))
            + block["cacheCreateTokens"] * rate.get("cacheWrite", rate.get("input", 0.0))
            + block["outputTokens"] * rate.get("output", 0.0)
        ) / 1_000_000
        priced["byModel"][model] = round(cost, 4)
        priced["total"] = round(priced["total"] + cost, 4)
    return priced


# ---------------------------------------------------------------------------
# Aggregation
# ---------------------------------------------------------------------------

def _empty_block() -> dict[str, int]:
    return {"requests": 0, "inputTokens": 0, "cacheReadTokens": 0, "cacheCreateTokens": 0,
            "outputTokens": 0, "thinkingTokens": 0, "billedTokens": 0}


def _add(block: dict[str, int], record: dict[str, Any]) -> None:
    block["requests"] += 1
    for key in ("inputTokens", "cacheReadTokens", "cacheCreateTokens", "outputTokens", "thinkingTokens"):
        block[key] += record[key]
    block["billedTokens"] += record["contextTokens"] + record["outputTokens"]


def summarize(records: list[dict[str, Any]], *, rates: dict[str, dict[str, float]] | None = None) -> dict[str, Any]:
    """Aggregate attributed turn records into a spend report."""
    totals = _empty_block()
    by_model: dict[str, dict[str, int]] = defaultdict(_empty_block)
    by_lane: dict[str, dict[str, int]] = defaultdict(_empty_block)
    by_tool: Counter[str] = Counter()
    by_tool_calls: Counter[str] = Counter()
    by_kind: Counter[str] = Counter()
    peak_context = 0

    for record in records:
        _add(totals, record)
        _add(by_model[record["model"]], record)
        _add(by_lane["subagent" if record["sidechain"] else "main"], record)
        peak_context = max(peak_context, record["contextTokens"])
        for item in record["attributed"]:
            tokens = item["attributedTokens"]
            by_kind[item["kind"]] += tokens
            if item["kind"] == "tool_result":
                by_tool[item["tool"] or "unknown"] += tokens
                by_tool_calls[item["tool"] or "unknown"] += 1

    growth_total = sum(by_kind.values())
    report: dict[str, Any] = {
        "schema": SCHEMA_VERSION,
        "honestyLimit": HONESTY_LIMIT,
        "totals": totals,
        "peakContextTokens": peak_context,
        "cacheReadShare": round(totals["cacheReadTokens"] / totals["billedTokens"], 4) if totals["billedTokens"] else 0.0,
        "byModel": {model: dict(block) for model, block in sorted(by_model.items())},
        "byLane": {lane: dict(block) for lane, block in sorted(by_lane.items())},
        "attributedGrowthTokens": growth_total,
        "attributedByKind": dict(by_kind.most_common()),
        "attributedByTool": [
            {"tool": tool, "attributedTokens": tokens, "results": by_tool_calls[tool],
             "share": round(tokens / growth_total, 4) if growth_total else 0.0}
            for tool, tokens in by_tool.most_common()
        ],
    }
    priced = apply_rates(report, rates)
    if priced is not None:
        report["estimatedCost"] = priced
    return report


# ---------------------------------------------------------------------------
# Waste findings
# ---------------------------------------------------------------------------

def _finding(slug: str, severity: str, observed: str, *, mechanism: str,
             remediations: list[str], evidence: list[dict[str, Any]],
             tokens: int) -> dict[str, Any]:
    payload = json.dumps({"d": slug, "e": sorted(
        (item.get("rowIndex", -1), item.get("tool", "")) for item in evidence)},
        sort_keys=True, ensure_ascii=False)
    return {
        "findingId": "ts_" + hashlib.sha256(payload.encode("utf-8")).hexdigest()[:24],
        "detector": slug, "severity": severity, "wastedTokens": tokens,
        "observed": observed, "likelyMechanism": "candidate: " + mechanism,
        "suggestedRemediations": remediations, "evidence": evidence[:8],
        "honestyLimit": HONESTY_LIMIT,
    }


def find_waste(records: list[dict[str, Any]], summary: dict[str, Any]) -> list[dict[str, Any]]:
    """Detect observable token waste. Every finding names its measured cost."""
    findings: list[dict[str, Any]] = []

    # 1. One tool result that cost a lot of context in a single turn.
    oversized: list[dict[str, Any]] = []
    for record in records:
        for item in record["attributed"]:
            if item["kind"] == "tool_result" and item["attributedTokens"] >= LARGE_CONTRIBUTION_TOKENS:
                oversized.append({"rowIndex": record["rowIndex"], "timestamp": record["timestamp"],
                                  "tool": item["tool"], "attributedTokens": item["attributedTokens"],
                                  "chars": item["chars"], "sessionId": record["sessionId"]})
    if oversized:
        oversized.sort(key=lambda item: -item["attributedTokens"])
        total = sum(item["attributedTokens"] for item in oversized)
        biggest = oversized[0]
        findings.append(_finding(
            "oversized_tool_result",
            "high" if biggest["attributedTokens"] >= HUGE_CONTRIBUTION_TOKENS else "medium",
            f"{len(oversized)} tool result(s) each added ≥{LARGE_CONTRIBUTION_TOKENS:,} tokens to context; "
            f"largest was {biggest['tool']} at ~{biggest['attributedTokens']:,} tokens",
            mechanism="an unbounded read/search returned its full output into context and stays "
                      "billed on every later turn of the session",
            remediations=["bound the call (head/tail, line ranges, grep filters, --max-count)",
                          "write large output to a file and read back only what is needed",
                          "delegate the wide read to a subagent and keep only its conclusion"],
            evidence=oversized, tokens=total))

    # 2. Identical tool call repeated — the second result is paid for twice.
    repeats: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for record in records:
        for item in record["attributed"]:
            digest = item.get("inputDigest")
            if item["kind"] == "tool_result" and digest:
                repeats[(item["tool"], digest)].append(
                    {"rowIndex": record["rowIndex"], "timestamp": record["timestamp"],
                     "tool": item["tool"], "attributedTokens": item["attributedTokens"],
                     "sessionId": record["sessionId"]})
    repeated_evidence: list[dict[str, Any]] = []
    wasted = 0
    for (tool, _digest), items in repeats.items():
        if len(items) < REPEAT_CALL_MIN:
            continue
        # The first call is legitimate; every repeat is duplicated context.
        wasted += sum(item["attributedTokens"] for item in items[1:])
        repeated_evidence.extend(items)
    if repeated_evidence and wasted > 0:
        findings.append(_finding(
            "duplicate_tool_call_cost", "medium" if wasted >= LARGE_CONTRIBUTION_TOKENS else "low",
            f"identical tool calls repeated within the session cost ~{wasted:,} tokens in duplicated results",
            mechanism="the same query was re-issued instead of reusing the result already in context",
            remediations=["re-read the earlier result in context before re-running a call",
                          "cache expensive command output in a scratch file"],
            evidence=sorted(repeated_evidence, key=lambda item: -item["attributedTokens"]),
            tokens=wasted))

    # 3. Cold cache: a large context rebuilt with no cache read at all.
    cold = [{"rowIndex": record["rowIndex"], "timestamp": record["timestamp"],
             "tool": "", "cacheCreateTokens": record["cacheCreateTokens"],
             "sessionId": record["sessionId"]}
            for record in records
            if record["cacheReadTokens"] == 0 and record["cacheCreateTokens"] >= COLD_CACHE_TOKENS]
    if len(cold) > 1:
        total = sum(item["cacheCreateTokens"] for item in cold)
        findings.append(_finding(
            "cold_cache_rebuild", "medium",
            f"{len(cold)} requests rebuilt ≥{COLD_CACHE_TOKENS:,} tokens of prompt cache with zero cache reads "
            f"(~{total:,} tokens written)",
            mechanism="the prompt prefix changed or the cache expired between requests, so the whole "
                      "context was re-written at cache-write price instead of read at cache-read price",
            remediations=["keep the stable prefix (system prompt, tool defs, memory) byte-identical across turns",
                          "avoid editing always-on context files mid-session",
                          "batch work so gaps between requests stay inside the cache TTL"],
            evidence=cold, tokens=total))

    # 4. One tool dominating all context growth.
    for entry in summary.get("attributedByTool", []):
        if entry["share"] >= DOMINANT_TOOL_SHARE and entry["attributedTokens"] >= LARGE_CONTRIBUTION_TOKENS:
            findings.append(_finding(
                "tool_dominates_context", "low",
                f"{entry['tool']} accounts for ~{entry['share']:.0%} of all attributed context growth "
                f"(~{entry['attributedTokens']:,} tokens over {entry['results']} results)",
                mechanism="one tool is the session's dominant cost centre; its output shape drives the bill",
                remediations=[f"narrow {entry['tool']} output at the call site",
                              "check whether a cheaper tool answers the same question"],
                evidence=[{"rowIndex": -1, "tool": entry["tool"],
                           "attributedTokens": entry["attributedTokens"], "share": entry["share"]}],
                tokens=entry["attributedTokens"]))
            break

    findings.sort(key=lambda item: -item["wastedTokens"])
    return findings


# ---------------------------------------------------------------------------
# Public entry points
# ---------------------------------------------------------------------------

def analyze(path: str | Path, *, rates: dict[str, dict[str, float]] | None = None,
            include_turns: bool = False) -> dict[str, Any]:
    """Full token-spend report for one transcript."""
    turns = parse_turns(path)
    records = attribute(turns)
    summary = summarize(records, rates=rates)
    summary["path"] = str(path)
    summary["findings"] = find_waste(records, summary)
    summary["findingCount"] = len(summary["findings"])
    summary["wastedTokens"] = sum(item["wastedTokens"] for item in summary["findings"])
    if include_turns:
        summary["turns"] = records
    return summary


def merge(reports: list[dict[str, Any]]) -> dict[str, Any]:
    """Combine per-session spend reports into one aggregate block."""
    totals = _empty_block()
    by_model: dict[str, dict[str, int]] = defaultdict(_empty_block)
    by_lane: dict[str, dict[str, int]] = defaultdict(_empty_block)
    by_tool: Counter[str] = Counter()
    by_tool_calls: Counter[str] = Counter()
    by_kind: Counter[str] = Counter()
    peak = 0
    findings: list[dict[str, Any]] = []
    for report in reports:
        for key, value in report["totals"].items():
            totals[key] += value
        for model, block in report["byModel"].items():
            for key, value in block.items():
                by_model[model][key] += value
        for lane, block in report["byLane"].items():
            for key, value in block.items():
                by_lane[lane][key] += value
        for entry in report["attributedByTool"]:
            by_tool[entry["tool"]] += entry["attributedTokens"]
            by_tool_calls[entry["tool"]] += entry["results"]
        for kind, tokens in report["attributedByKind"].items():
            by_kind[kind] += tokens
        peak = max(peak, report["peakContextTokens"])
        findings.extend(report["findings"])
    growth_total = sum(by_kind.values())
    findings.sort(key=lambda item: -item["wastedTokens"])
    return {
        "schema": SCHEMA_VERSION, "honestyLimit": HONESTY_LIMIT,
        "sessionCount": len(reports), "totals": totals, "peakContextTokens": peak,
        "cacheReadShare": round(totals["cacheReadTokens"] / totals["billedTokens"], 4) if totals["billedTokens"] else 0.0,
        "byModel": {model: dict(block) for model, block in sorted(by_model.items())},
        "byLane": {lane: dict(block) for lane, block in sorted(by_lane.items())},
        "attributedGrowthTokens": growth_total,
        "attributedByKind": dict(by_kind.most_common()),
        "attributedByTool": [
            {"tool": tool, "attributedTokens": tokens, "results": by_tool_calls[tool],
             "share": round(tokens / growth_total, 4) if growth_total else 0.0}
            for tool, tokens in by_tool.most_common()],
        "findings": findings, "findingCount": len(findings),
        "wastedTokens": sum(item["wastedTokens"] for item in findings),
    }


def render_text(report: dict[str, Any]) -> str:
    """Human-readable summary of a spend report (or a merged one)."""
    totals = report["totals"]
    lines = [
        "TOKEN SPEND",
        f"  requests        {totals['requests']:>12,}",
        f"  billed total    {totals['billedTokens']:>12,}",
        f"    fresh input   {totals['inputTokens']:>12,}",
        f"    cache read    {totals['cacheReadTokens']:>12,}  ({report['cacheReadShare']:.0%} of billed)",
        f"    cache write   {totals['cacheCreateTokens']:>12,}",
        f"    output        {totals['outputTokens']:>12,}  (thinking {totals['thinkingTokens']:,})",
        f"  peak context    {report['peakContextTokens']:>12,}",
    ]
    if report.get("estimatedCost"):
        lines.append(f"  estimated cost  {report['estimatedCost']['total']:>12,.2f}  (caller-supplied rates)")
    if len(report.get("byModel", {})) > 1:
        lines.append("BY MODEL")
        for model, block in sorted(report["byModel"].items(), key=lambda item: -item[1]["billedTokens"]):
            lines.append(f"  {model:<34} {block['billedTokens']:>12,}")
    if report.get("byLane"):
        lines.append("BY LANE")
        for lane, block in sorted(report["byLane"].items()):
            lines.append(f"  {lane:<34} {block['billedTokens']:>12,}")
    lines.append(f"WHERE CONTEXT GREW  (attributed, heuristic — {report['attributedGrowthTokens']:,} tokens)")
    for kind, tokens in report["attributedByKind"].items():
        lines.append(f"  {kind:<34} {tokens:>12,}")
    if report["attributedByTool"]:
        lines.append("TOP TOOLS BY CONTEXT ADDED")
        for entry in report["attributedByTool"][:10]:
            lines.append(f"  {entry['tool']:<28} {entry['attributedTokens']:>12,}"
                         f"  {entry['share']:>5.0%}  ({entry['results']} results)")
    lines.append(f"WASTE FINDINGS  ({report['findingCount']}, ~{report['wastedTokens']:,} tokens implicated)")
    for finding in report["findings"]:
        lines.append(f"  [{finding['severity']}] {finding['detector']}: {finding['observed']}")
        for remediation in finding["suggestedRemediations"][:2]:
            lines.append(f"      → {remediation}")
    lines.append(f"NOTE: {report['honestyLimit']}")
    return "\n".join(lines)


def cli_token_spend(argv: list[str] | None = None) -> int:
    """``adapt token-spend <transcript...>`` — report only, writes nothing."""
    import argparse
    import sys

    ap = argparse.ArgumentParser(prog="adapt token-spend")
    ap.add_argument("transcript", nargs="+", help="one or more Claude Code or Codex JSONL transcripts")
    ap.add_argument("--json", action="store_true", help="emit the JSON report instead of the text table")
    ap.add_argument("--turns", action="store_true", help="include per-turn records in JSON output")
    ap.add_argument("--rates", default=None,
                    help="JSON file of model -> {input,output,cacheRead,cacheWrite} per million tokens")
    ap.add_argument("--out", default=None, help="write the report here instead of stdout")
    args = ap.parse_args(argv)

    rates = None
    if args.rates:
        rates = json.loads(Path(args.rates).read_text(encoding="utf-8"))

    reports = [analyze(path, rates=rates, include_turns=args.turns) for path in args.transcript]
    merged = merge(reports) if len(reports) != 1 else reports[0]
    if len(reports) > 1:
        merged["sessions"] = [{k: v for k, v in report.items() if k != "turns"} for report in reports]
    if rates:
        priced = apply_rates(merged, rates)
        if priced is not None:
            merged["estimatedCost"] = priced

    encoded = json.dumps(merged, indent=2, sort_keys=True, ensure_ascii=False, default=str) if args.json \
        else render_text(merged)
    if args.out:
        Path(args.out).write_text(encoded, encoding="utf-8")
        print(f"adapt token-spend: wrote report -> {args.out}", file=sys.stderr)
    else:
        print(encoded)
    return 0


if __name__ == "__main__":
    raise SystemExit(cli_token_spend())
