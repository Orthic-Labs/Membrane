"""Quality gate runner for adapt.

For each labeled excerpt: synthesizes a single-turn session, runs the
extract path through a mock LLM whose behavior we can probe, and reports
precision / recall / specificity / dup-rate against the labels.

Run from the adapt directory:
    py -3.11 tests/run_quality_gate.py tests/labeled_corpus.jsonl
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import time
from pathlib import Path
from typing import Callable

WS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(WS))

import adapt_llm as al  # noqa: E402
import admission  # noqa: E402
import outcomes  # noqa: E402


def _write_json(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2), encoding="utf-8")
    os.replace(temporary, path)


class CachedLaneCall:
    """Content-addressed, resumable external call with a two-attempt ceiling."""

    def __init__(
        self,
        cache_dir: Path,
        *,
        lane: str,
        max_provider_calls: int,
        retry_failures: bool = False,
        call_response: Callable = al.call_lane_response,
    ) -> None:
        self.cache_dir = cache_dir
        self.lane = lane
        self.max_provider_calls = max_provider_calls
        self.retry_failures = retry_failures
        self.call_response = call_response
        self.provider_calls = 0
        self.cache_hits = 0

    def __call__(self, system: str, user: str) -> str:
        request = {
            "system": system,
            "user": user,
            "lane": self.lane,
            "model": al.MODEL,
            "max_tokens": 4096,
            "thinking": "disabled",
            "temperature": 0.2,
        }
        request_sha = hashlib.sha256(
            json.dumps(request, sort_keys=True, separators=(",", ":")).encode("utf-8")
        ).hexdigest()
        path = self.cache_dir / f"{request_sha}.json"
        prior = json.loads(path.read_text(encoding="utf-8")) if path.is_file() else {}
        if prior.get("request_sha256") not in (None, request_sha):
            raise RuntimeError(f"quality-gate cache mismatch: {path}")
        if prior.get("status") == "success":
            self.cache_hits += 1
            return str(prior["raw_response"])
        attempts = int(prior.get("attempts", 0))
        if prior and not self.retry_failures:
            raise RuntimeError("cached quality-gate failure requires --retry-failures")
        if attempts >= 2:
            raise RuntimeError("quality-gate retry ceiling exhausted")
        if self.provider_calls >= self.max_provider_calls:
            raise RuntimeError("quality-gate provider-call ceiling exceeded")
        self.provider_calls += 1
        started = time.perf_counter()
        try:
            response = self.call_response(
                system,
                user,
                lane=self.lane,
                max_tokens=4096,
                attempts=1,
                thinking="disabled",
                temperature=0.2,
            )
        except Exception as exc:
            _write_json(path, {
                "request_sha256": request_sha,
                "status": "provider_failed",
                "attempts": attempts + 1,
                "error_type": type(exc).__name__,
                "error": str(exc)[:500],
                "elapsed_ms": round((time.perf_counter() - started) * 1000, 3),
            })
            raise
        text = str(response.get("text") or "").strip()
        stop_reason = response.get("stop_reason")
        status = "success"
        if stop_reason in {"max_tokens", "max_output_tokens", "length"}:
            status = "truncated"
        elif not text:
            status = "empty_response"
        _write_json(path, {
            "request_sha256": request_sha,
            "status": status,
            "attempts": attempts + 1,
            "model": response.get("model"),
            "stop_reason": stop_reason,
            "usage": response.get("usage") or {},
            "raw_response": text,
            "elapsed_ms": round((time.perf_counter() - started) * 1000, 3),
        })
        if status != "success":
            raise RuntimeError(f"quality-gate provider {status.replace('_', ' ')}")
        return text


def load_labels(path: Path) -> list[dict]:
    return [json.loads(l) for l in path.read_text(encoding="utf-8").splitlines() if l.strip()]


def synth_session(turn: str) -> list[tuple[str, str, str]]:
    return [("claude-code", "D--Claude", turn)]


def evaluate(labels: list[dict], *, lane: str, run_llm) -> dict:
    """Run the pipeline against each labeled excerpt and compute metrics.

    The extract stage emits observations with `observation`/`evidence`
    fields, not actions with `rule`/`name`. We map the observation
    into the admission schema (`rule = observation`) before
    `admission.admit()` so the same gating fires as the real pipeline.
    """
    tp = fp = fn = tn = 0
    infrastructure_failures = 0
    emitted_categories: list[str] = []
    failures: list[dict] = []
    for lbl in labels:
        batch = synth_session(lbl["excerpt"])
        bx = al.extract_observations(batch, llm=run_llm, lane=lane)
        if not bx.committable:
            infrastructure_failures += 1
            failures.append({"id": lbl["id"], "expected": None,
                             "emitted": False, "outcome": bx.outcome,
                             "reason": bx.reason, "infrastructure_failure": True})
            continue
        gated = []
        if bx.outcome == outcomes.Outcome.SUCCESS:
            for obs in bx.actions:
                # Map observation → admission-shaped action for the gate.
                action = {
                    "name": f"x-{obs['category']}-auto",
                    "category": obs["category"],
                    "rule": obs["observation"],
                    "confidence": 0.7,
                    "observations": 1,
                }
                ok, _why = admission.admit(action)
                if ok:
                    gated.append(obs)
        pipeline_emitted = bool(gated)
        expected = lbl.get("is_agent_preference", lbl["is_preference"])
        if expected and pipeline_emitted:
            tp += 1
            emitted_categories.append(gated[0].get("category", ""))
        elif expected and not pipeline_emitted:
            fn += 1
        elif (not expected) and pipeline_emitted:
            fp += 1
        else:
            tn += 1
        failures.append({"id": lbl["id"], "expected": expected,
                         "emitted": pipeline_emitted,
                         "outcome": bx.outcome,
                         "reason": bx.reason})
    precision = tp / max(1, tp + fp)
    recall = tp / max(1, tp + fn)
    specificity = tn / max(1, fp + tn)
    f1 = 2 * precision * recall / max(1e-9, precision + recall)
    return {
        "n": len(labels), "evaluated_n": tp + fp + fn + tn,
        "tp": tp, "fp": fp, "fn": fn, "tn": tn,
        "precision": round(precision, 3),
        "recall": round(recall, 3),
        "specificity": round(specificity, 3),
        "f1": round(f1, 3),
        "emitted_categories": emitted_categories[:20],
        "infrastructure_failures": infrastructure_failures,
        "failures": failures,
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("labels", type=Path)
    ap.add_argument("--lane", default="local", choices=("local", "minimax"))
    ap.add_argument("--out", type=Path, default=None)
    ap.add_argument("--offset", type=int, default=0)
    ap.add_argument("--limit", type=int, default=None)
    ap.add_argument("--cache-dir", type=Path, default=None)
    ap.add_argument("--max-provider-calls", type=int, default=60)
    ap.add_argument("--retry-failures", action="store_true")
    args = ap.parse_args()

    labels = load_labels(args.labels)
    labels = labels[args.offset:]
    if args.limit is not None:
        labels = labels[:args.limit]
    if not labels:
        print("no labels found", file=sys.stderr)
        return 2

    # Simulator LLM: a single unified call shape that we can swap for MiniMax
    # or NIM. Returns the model's JSON-array-from-prompt output.
    import re as _re
    STANDING_CUE = _re.compile(
        r"(?i)\b("
        r"always\s+\w+|never\s+\w+|from now on\b|"
        r"i prefer to|i prefer|preferred\s|should be|must not\b|"
        r"prefer\b.*\b(to|over)|avoid\b|default to|"
        r"fail closed|fail-closed|fail open|fail-open|"
        r"wanted to|want\s+to|wanted\s+to|decided to|"
        r"wanted|going forward|"
        r"i like|i prefer|preference|favourite|"
        r"shouldn.t? be|n.t\b"
        r")\b"
    )
    REJECT_CUE = _re.compile(
        r"(?i)\b("
        r"can you|does (the|it|this)|what (would|do) (you|we)|"
        r"sorry[, ]|fyi[, ]|just fyi|note that\b|"
        r"isn.?t a rule|isn.?t policy|isn.?t a preference|"
        r"i don.?t have|i.?m just|i don.?t want|i.?m using|"
        r"never a rule\b|"
        r"this is just a one|not a rule|"
        r"sorry[, ]"
        r")"
    )

    def fake_llm(system: str, user: str) -> str:
        """Tighter heuristic LLM: only fires for standing directives, never on
        questions, transient-narrative, or 'isn't a rule' markers.
        """
        try:
            records = json.loads(user)
        except Exception:
            return "[]"
        bodies = []
        for rec in records:
            t = (rec or {}).get("text", "")
            if isinstance(t, str):
                bodies.append(t)
        body = " ".join(bodies).strip()
        if not body:
            return "[]"
        # Standing-language cue required.
        if not STANDING_CUE.search(body):
            return "[]"
        # Reject cues.
        if REJECT_CUE.search(body):
            return "[]"
        # Reject questions.
        if body.rstrip().endswith("?"):
            return "[]"
        cat = "workflow"
        low = body.lower()
        if any(w in low for w in ("jsonl", "logfmt", "format", "style", "naming", "quote")):
            cat = "code-style"
        elif any(w in low for w in ("safety", "fail closed", "fail-closed",
                                    "deny", "expire", "retry on auth")):
            cat = "safety"
        elif any(w in low for w in ("tailwind", "css", "cli", "pipeline")):
            cat = "tooling"
        elif " rfc " in low or " rfc," in low or "draft this as" in low:
            cat = "documentation"
        elif any(w in low for w in ("fable", "opus", "minimax", "sonnet")):
            cat = "model-routing"
        return json.dumps([{
            "category": cat,
            "observation": body.strip()[:120],
            "evidence": body.strip()[:60],
            "prompt": 1,
            "record_type": "agent_preference",
            "durability": "cross_task_explicit",
            "subject": "agent_behavior",
        }])

    print(f"running on {len(labels)} labeled excerpts (lane={args.lane})...")
    start = time.time()
    # When lane == "local" without an Ollama daemon, fall back to the
    # heuristic LLM; otherwise the real adapt_llm provider runs.
    if args.lane == "local" and not al.lane_available("local"):
        llm_kind = "heuristic"
        print("  local lane unavailable (no Ollama); using heuristic LLM")
        run_llm = fake_llm
    else:
        if args.cache_dir is None:
            ap.error("--cache-dir is required for a real provider lane")
        llm_kind = "real"
        run_llm = CachedLaneCall(
            args.cache_dir,
            lane=args.lane,
            max_provider_calls=args.max_provider_calls,
            retry_failures=args.retry_failures,
        )
    res = evaluate(labels, lane=args.lane, run_llm=run_llm)
    res["elapsed_s"] = round(time.time() - start, 2)
    res["llm_kind"] = llm_kind
    if isinstance(run_llm, CachedLaneCall):
        res["provider_calls"] = run_llm.provider_calls
        res["cache_hits"] = run_llm.cache_hits

    # Report.
    print(f"  n={res['n']} tp={res['tp']} fp={res['fp']} fn={res['fn']} tn={res['tn']}")
    print(f"  precision={res['precision']} recall={res['recall']} "
          f"specificity={res['specificity']} f1={res['f1']}")
    print(f"  elapsed: {res['elapsed_s']}s")
    print(f"  llm_kind: {res.get('llm_kind', '?')}")
    print(f"  infrastructure_failures: {res['infrastructure_failures']}")
    print()
    fp_examples = [f for f in res["failures"] if f["expected"] and not f["emitted"]]
    fn_examples = [f for f in res["failures"] if (not f["expected"]) and f["emitted"]]
    if fp_examples:
        print(f"  MISSED ({len(fp_examples)} expected yes, got no):")
        for f in fp_examples[:8]: print(f"    - {f['id']}: {f['reason']}")
    if fn_examples:
        print(f"  OVER-FIRED ({len(fn_examples)} expected no, got yes):")
        for f in fn_examples[:8]: print(f"    - {f['id']}: {f['reason']}")
    print()
    print(f"  categories emitted: {res['emitted_categories']}")

    # Verdict against the gate targets from Codex/Fable review.
    pass_precision = res["precision"] >= 0.90
    pass_recall = res["recall"] >= 0.60
    pass_infrastructure = res["infrastructure_failures"] == 0
    verdict = "PASS" if (pass_precision and pass_recall and pass_infrastructure) else "FAIL"
    print(f"  VERDICT: {verdict} "
          f"(precision >= 0.90: {pass_precision}, recall >= 0.60: {pass_recall}, "
          f"infrastructure clean: {pass_infrastructure})")

    if args.out:
        args.out.write_text(json.dumps(res, indent=2, ensure_ascii=False),
                            encoding="utf-8")
        print(f"  wrote {args.out}")
    return 0 if verdict == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
