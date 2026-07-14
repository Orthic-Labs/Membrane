"""Gate 0 step 2 — emit a 10-session Adapt candidate manifest for arm D.

This script bypasses the live LLM lane by accepting canned ``extract`` and
``synthesize`` payloads from a JSON fixture, runs the rest of the Adapt
pipeline (canned observations → synthesize → apply_actions manifest emission)
with the new PreferenceRecord + manifest schema, and writes a schema-valid
manifest to ``<out>``.

Why fixture-driven instead of a real LLM run: a real emission costs tokens,
takes minutes, and is data-dependent on sessions you may not want re-mined
right now. The fixture path proves the structural contract end-to-end and
produces an artifact ``run_value_ab.py --adapt-manifest`` can consume.

Switching from fixture to real LLM later is a one-line swap of the
``extract_fn`` + ``synth_fn`` call sites for ``adapt_llm.extract_observations``
+ ``adapt_llm.synthesize``. The rest of the path is unchanged.

Usage::

    py -3.11 tools/pipelines/memory/adapt/eval/emit_arm_d_candidates.py \\
        --fixture path/to/fixture.json \\
        --out D:/Claude/.cache/adapt/review/10session.manifest.json

Fixture shape::

    {
      "observations": [
        {"category": "tooling", "observation": "...",
         "evidence": "...", "prompt": 1, "tool": "claude-code",
         "scope": "D--Claude"}
      ],
      "synthesis_response": [
        {"action": "add", "name": "ignored",
         "category": "tooling", "rule": "Use JSONL for structured logs.",
         "confidence": 0.7, "observations": 1, "why": "stated by operator"}
      ],
      "source_session_ids": ["s1", "s2", "s3", "s4", "s5",
                             "s6", "s7", "s8", "s9", "s10"]
    }

The fixture need not contain exactly 10 sessions; the count is for label
provenance, not engine semantics.
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import sys
import tempfile
from pathlib import Path
from collections import defaultdict

WS = Path(__file__).resolve().parents[5]  # workspace root — hardcoded D:/Claude broke non-Windows checkouts
sys.path.insert(0, str(WS / "tools/pipelines/memory/adapt"))

import preference_record as pr_mod  # noqa: E402
import manifest  # noqa: E402
import adapt  # noqa: E402
import authority  # noqa: E402


def _default_fixture() -> dict:
    """A small, plausible canned observation + synthesis set.

    Mirrors the standing-language patterns the LLM is supposed to keep; the
    rules themselves are short imperative preferences with categories from
    the controlled taxonomy.
    """
    return {
        "observations": [
            {"category": "tooling", "observation": "use JSONL for structured logs",
             "evidence": "always use JSONL for structured logging", "prompt": 1,
             "tool": "claude-code", "scope": "D--Claude",
             "source": "model"},
            {"category": "verification", "observation": "run smoke tests before merges",
             "evidence": "always run smoke gate on greenfield change", "prompt": 2,
             "tool": "claude-code", "scope": "D--Claude",
             "source": "model"},
            {"category": "workflow", "observation": "validate plan before implementation",
             "evidence": "validate the plan first before going broad", "prompt": 3,
             "tool": "claude-code", "scope": "D--Claude",
             "source": "model"},
            {"category": "safety", "observation": "refuse destructive prod-db commands",
             "evidence": "never run raw DROP on the live vendure DB", "prompt": 4,
             "tool": "claude-code", "scope": "D--Claude",
             "source": "model"},
        ],
        "synthesis_response": [
            {"action": "add", "name": "ignored",
             "category": "tooling",
             "rule": "Use JSONL for structured logs, never logfmt, in this codebase.",
             "confidence": 0.7, "observations": 1, "why": "operator stated directly"},
            {"action": "add", "name": "ignored",
             "category": "verification",
             "rule": "Run the smoke gate on every greenfield change before merging.",
             "confidence": 0.75, "observations": 1, "why": "stated as standing rule"},
            {"action": "add", "name": "ignored",
             "category": "workflow",
             "rule": "Validate the plan with the user before any broad implementation.",
             "confidence": 0.8, "observations": 1, "why": "explicit standing rule"},
            {"action": "add", "name": "ignored",
             "category": "safety",
             "rule": "Refuse raw DROP or TRUNCATE on the live vendure-postgres container.",
             "confidence": 0.85, "observations": 1, "why": "explicit safety directive"},
        ],
        "source_session_ids": [f"s{i}" for i in range(1, 11)],
    }


def _emit_from_fixture(fx: dict, out_path: Path) -> dict:
    """Run the structural pipeline against a fixture and write the manifest.

    The output manifest matches the Gate 1a schema and is what
    ``run_value_ab.py --adapt-manifest`` consumes.
    """
    observations = list(fx["observations"])
    synth_actions = list(fx["synthesis_response"])
    source_session_ids = list(fx["source_session_ids"])
    authority_snapshot = authority.build_manifest(WS)

    # Wrap synthesis actions through PreferenceRecord to derive ids + apply
    # admission. admission.admit() refuses rules outside the controlled
    # categories and rules shorter than 8 words.
    import admission  # local import
    canonical_rules: set[str] = set()
    rules: dict[str, dict] = {}
    manifest_records: list[dict] = []
    obs_by_cat: dict[str, list[dict]] = defaultdict(list)
    for o in observations:
        obs_by_cat[o["category"]].append(o)

    for act in synth_actions:
        admitted, why = admission.admit(
            {**act, "scope": SCOPE},
            canonical_rules=canonical_rules,
            authority_manifest=authority_snapshot,
            authority_root=WS,
        )
        pr = pr_mod.PreferenceRecord.from_synthesis(
            act, scope=SCOPE, source_ids=tuple(source_session_ids),
        )
        canonical_rules.add(pr.id)
        rules[pr.id] = {
            "name": pr.id, "category": pr.category, "rule": pr.rule,
            "confidence": pr.confidence, "observations": pr.evidence_count,
            "scope": pr.scope, "needs_review": pr.needs_review,
            "record_type": pr.record_type,
            "authority_effect": pr.authority_effect,
        }
        rec = pr_mod.to_manifest_candidate(
            pr, evidence_excerpt=(obs_by_cat[act["category"]][0]["evidence"]
                                   if obs_by_cat[act["category"]] else ""),
            status="pending",
        )
        rec["authority_manifest_sha256"] = authority_snapshot["manifest_sha256"]
        rec["payload_sha256"] = manifest.payload_sha256(rec)
        rec["_admission_status"] = "pending" if admitted else "rejected-by-admission"
        rec["_rejection_reason"] = None if admitted else why
        rec["_action"] = act.get("action", "add")
        manifest_records.append(rec)

    # Schema-shaped records (strip admission metadata).
    schema_records = [{k: v for k, v in r.items() if not k.startswith("_")}
                      for r in manifest_records]
    # Mark admission-rejected as 'rejected' so reviewers see the gate decision.
    for r, raw in zip(schema_records, manifest_records):
        if raw.get("_admission_status") == "rejected-by-admission":
            r["status"] = "rejected"
            if raw.get("_rejection_reason"):
                r["human_note"] = f"admission-rejected: {raw['_rejection_reason']}"

    body = {
        "schema_version": pr_mod.SCHEMA_VERSION,
        "batch_id": f"fixture-{dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%S')}",
        "created_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "generator": "emit_arm_d_candidates.py (fixture-driven)",
        "authority_manifest": authority_snapshot,
        "source_session_ids": source_session_ids,
        "records": schema_records,
    }
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(body, indent=2, ensure_ascii=False),
                        encoding="utf-8")
    return body


# Late-bound so monkeypatch of adapt.ts.STATE_DIR still works if anyone
# imports this as a library and swaps state dirs.
SCOPE = "D--Claude"


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--fixture", type=Path, default=None,
                    help="path to a JSON fixture; defaults to a built-in sample")
    ap.add_argument("--out", type=Path, required=True,
                    help="where to write the schema-conformant manifest")
    ap.add_argument("--print-summary", action="store_true",
                    help="print accepted/pending/rejected counts and exit")
    args = ap.parse_args(argv)

    if args.fixture:
        fx = json.loads(args.fixture.read_text(encoding="utf-8"))
    else:
        fx = _default_fixture()

    written = _emit_from_fixture(fx, args.out)

    if args.print_summary:
        rows = written["records"]
        n_acc = sum(1 for r in rows if r["status"] == "accepted")
        n_rej = sum(1 for r in rows if r["status"] == "rejected")
        n_pend = sum(1 for r in rows if r["status"] == "pending")
        print(f"wrote {args.out}")
        print(f"  batch_id: {written['batch_id']}")
        print(f"  records: {len(rows)} "
              f"(pending={n_pend}, accepted={n_acc}, rejected={n_rej})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
