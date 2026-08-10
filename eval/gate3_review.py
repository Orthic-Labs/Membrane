"""Gate 3 — review driver for a 10-session Morph candidate manifest.

The v2 plan's Gate 3 requires that every accepted candidate passes deterministic
checks before any live apply, and that unsupported rules (no source excerpt
that entails the imperative meaning) are a hard failure.

What this driver checks (deterministic, no LLM):

  - evidence_fidelity  rule text contains at least one verb/noun from the
                       bounded evidence excerpt (a heuristic, not a proof —
                       a real grader reviews; this is the gate that catches
                       fabricated rules with random evidence).
  - durability         rule body has ≥ 8 words and an imperative verb.
  - category           is in admission.ALLOWED_CATEGORIES.
  - scope              scope matches the controlled scope taxonomy (D--Claude*).
  - exact_duplication  normalized-rule-body equality across accepted set.
  - overbroad          evidence_excerpt > 200 chars OR rule > 50 words (heuristic
                       that catches pipeline-emitted run-on sentences).

What this driver does NOT check (LLM-required; deferred to a separate grader):

  - semantic_duplication (cosine > 0.97 on embedding space)
  - contradiction across the accepted set
  - subjective evidence fidelity beyond the keyword heuristic

Outputs::

    <out>/gate3.report.json    structured per-record review
    <out>/gate3.report.md      human-readable summary
    The driver also EXITS NON-ZERO if any ``unsupported_rules`` count > 0,
    since the v2 plan makes that a hard failure.
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import re
import sys
from collections import defaultdict
from pathlib import Path


WS = next(p for p in Path(__file__).resolve().parents if (p / "tools" / "lib").is_dir())  # workspace root: the dir that owns tools/lib (never a fixed parent depth)
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import admission  # noqa: E402
import preference_record as pr_mod  # noqa: E402

# Heuristic: imperative verbs that imply a real directive.
_IMPERATIVE_VERBS = {
    "always", "never", "prefer", "use", "avoid", "refuse", "validate",
    "confirm", "require", "ensure", "mark", "limit", "capture", "run",
    "stop", "restart", "edit", "write", "read", "open", "close", "split",
    "merge", "build", "test", "deploy", "verify", "log", "check", "include",
    "exclude", "tag", "label", "flush", "warm", "lock", "release", "delete",
    "scan", "load", "save", "inject", "store", "emit", "reject", "refactor",
    "rename", "freeze", "purge", "normalize", "route", "bind", "accept",
}

_MAX_RULE_WORDS = 50
_MAX_EXCERPT_CHARS = 200


def _word_set(text: str) -> set[str]:
    return {w.casefold().strip(".,;:!?()[]\"'") for w in text.split() if w}


def evidence_fidelity(rule: str, excerpt: str) -> tuple[bool, str]:
    """Heuristic: at least one imperative verb OR key noun appears in both."""
    if not excerpt:
        return False, "no excerpt"
    r_words = _word_set(rule)
    e_words = _word_set(excerpt)
    # If rule contains an imperative verb from our list AND that verb appears
    # in the excerpt (case-insensitive), the rule is evidence-grounded.
    overlap = r_words & e_words
    shared_imperative = (r_words & _IMPERATIVE_VERBS) & e_words
    if shared_imperative:
        return True, "shared imperative verb"
    if len(overlap) >= 2:
        return True, f"shared {len(overlap)} word(s)"
    return False, "no shared verb/noun between rule and excerpt"


def durability_ok(rule: str) -> tuple[bool, str]:
    words = rule.split()
    if len(words) < 8:
        return False, f"rule too short ({len(words)} words)"
    if not (set(w.casefold() for w in words) & _IMPERATIVE_VERBS):
        return False, "no imperative verb"
    return True, "ok"


def category_ok(category: str) -> tuple[bool, str]:
    if category not in admission.ALLOWED_CATEGORIES:
        return False, f"category '{category}' outside controlled taxonomy"
    return True, "ok"


def scope_ok(scope: str) -> tuple[bool, str]:
    # Allowed scopes follow D--Claude[-...] pattern (the engine's namespace).
    if not scope:
        return False, "empty scope"
    if not scope.startswith("D--Claude"):
        return False, f"scope '{scope}' outside D--Claude* namespace"
    return True, "ok"


def overbroad_ok(rule: str, excerpt: str) -> tuple[bool, str]:
    """Returns (ok, reason). ok=False means rule/excerpt is overbroad."""
    if len(rule.split()) > _MAX_RULE_WORDS:
        return False, f"rule > {_MAX_RULE_WORDS} words"
    if len(excerpt) > _MAX_EXCERPT_CHARS:
        return False, f"excerpt > {_MAX_EXCERPT_CHARS} chars"
    return True, "bounded"


def _normalize_rule(rule: str) -> str:
    return pr_mod.normalize_rule(rule)


def review_manifest(manifest: dict) -> dict:
    """Run the deterministic review over every record in a manifest."""
    accepted = [r for r in manifest.get("records", [])
                if r.get("status") == "accepted"]
    pending = [r for r in manifest.get("records", [])
               if r.get("status") == "pending"]

    # Exact-duplicate index over accepted (and pending, separately).
    def _dup_index(records: list[dict]) -> dict[str, list[str]]:
        bucket: dict[str, list[str]] = defaultdict(list)
        for r in records:
            bucket[_normalize_rule(r.get("rule", ""))].append(r["id"])
        return {k: v for k, v in bucket.items() if len(v) > 1}

    accepted_dups = _dup_index(accepted)
    pending_dups = _dup_index(pending)

    reviews = []
    unsupported = 0
    rejected_by_gate = 0
    for r in accepted + pending:
        rule = r.get("rule", "")
        excerpt = r.get("evidence_excerpt", "")
        checks = {
            "evidence_fidelity": evidence_fidelity(rule, excerpt),
            "durability": durability_ok(rule),
            "category": category_ok(r.get("category", "")),
            "scope": scope_ok(r.get("scope", "")),
            "overbroad": overbroad_ok(rule, excerpt),
        }
        failed = [name for name, (ok, _why) in checks.items() if not ok]
        is_unsupported = (
            not checks["evidence_fidelity"][0]
            or not checks["durability"][0]
            or not checks["category"][0]
            or not checks["scope"][0]
        )
        if is_unsupported:
            unsupported += 1
        if failed and r.get("status") == "accepted":
            rejected_by_gate += 1
        # Duplication note (only flagged when in the same set).
        dup_with = []
        if r.get("status") == "accepted":
            dup_bucket = accepted_dups.get(_normalize_rule(rule), [])
        else:
            dup_bucket = pending_dups.get(_normalize_rule(rule), [])
        dup_with = [x for x in dup_bucket if x != r["id"]]
        reviews.append({
            "id": r["id"],
            "status": r.get("status", ""),
            "rule": rule,
            "checks": {k: list(v) for k, v in checks.items()},
            "failed_checks": failed,
            "unsupported": is_unsupported,
            "exact_duplicate_of": dup_with,
        })

    return {
        "generated_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "batch_id": manifest.get("batch_id", ""),
        "counts": {
            "accepted": len(accepted),
            "pending": len(pending),
            "rejected": len([r for r in manifest.get("records", [])
                              if r.get("status") == "rejected"]),
            "unsupported": unsupported,
            "rejected_by_gate": rejected_by_gate,
            "exact_duplicates_accepted": sum(1 for v in accepted_dups.values()
                                              for _ in v),
            "exact_duplicates_pending": sum(1 for v in pending_dups.values()
                                              for _ in v),
        },
        "accepted_duplicates": accepted_dups,
        "pending_duplicates": pending_dups,
        "per_record": reviews,
    }


def _format_report(review: dict) -> str:
    L = ["# Gate 3 — manifest review (deterministic)", ""]
    L.append(f"- batch_id: {review['batch_id']}")
    L.append(f"- generated_at: {review['generated_at']}")
    L.append("")
    L.append("## Counts")
    L.append("")
    for k, v in review["counts"].items():
        L.append(f"- {k}: {v}")
    L.append("")
    L.append("## Per-record verdict")
    L.append("")
    L.append("| id | status | unsupported | failed checks | exact_dup_of |")
    L.append("|---|---|---|---|---|")
    for r in review["per_record"]:
        L.append(f"| {r['id']} | {r['status']} | "
                 f"{'YES' if r['unsupported'] else 'no'} | "
                 f"{','.join(r['failed_checks']) or '—'} | "
                 f"{','.join(r['exact_duplicate_of']) or '—'} |")
    L.append("")
    if review["counts"]["unsupported"]:
        L.append("## Hard failure")
        L.append("")
        L.append("The v2 plan requires zero unsupported rules. The records "
                 "marked `unsupported` above MUST be regenerated or rejected "
                 "before any apply.")
    return "\n".join(L) + "\n"


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--manifest", type=Path, required=True,
                    help="the reviewed Morph candidate manifest to gate")
    ap.add_argument("--out", type=Path, required=True,
                    help="where to write gate3.report.json + gate3.report.md")
    args = ap.parse_args(argv)

    if not args.manifest.exists():
        print(f"error: manifest not found: {args.manifest}", file=sys.stderr)
        return 2
    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    review = review_manifest(manifest)
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "gate3.report.json").write_text(
        json.dumps(review, indent=2, ensure_ascii=False), encoding="utf-8")
    (args.out / "gate3.report.md").write_text(_format_report(review),
                                              encoding="utf-8")
    print(f"report: {args.out / 'gate3.report.md'}")
    print(f"counts: {json.dumps(review['counts'])}")
    # Hard fail if any unsupported rule leaked through.
    if review["counts"]["unsupported"]:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())