#!/usr/bin/env python3
"""Reapply deterministic scope provenance to an existing compiler result."""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import sys
from collections import Counter
from pathlib import Path
from typing import Sequence


EVAL_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(EVAL_DIR))
import run_sequential_self_merge as compiler


def _source_rows(rows: Sequence[dict]) -> dict[str, list[dict]]:
    sources: dict[str, list[dict]] = {}
    for row in rows:
        session_id = row.get("session_id")
        if isinstance(session_id, str):
            sources.setdefault(session_id, []).append(row)
    return sources


def _matched_rows(rule: dict, sources: dict[str, list[dict]]) -> list[dict]:
    matched: list[dict] = []
    support = rule.get("support")
    if not isinstance(support, list) or not support:
        return matched
    for citation in support:
        if not isinstance(citation, dict):
            return []
        session_id = citation.get("session_id")
        evidence = citation.get("evidence")
        if not isinstance(session_id, str) or not isinstance(evidence, str):
            return []
        matches = [
            row for row in sources.get(session_id, [])
            if isinstance(row.get("text"), str)
            and compiler._evidence_matches(evidence, row["text"])
        ]
        if not matches:
            return []
        matched.extend(matches)
    return matched


def rescope_report(
    report: dict,
    *,
    rows: Sequence[dict],
    scope_catalog: dict[str, str],
    workspace_scope: str,
) -> dict:
    output = copy.deepcopy(report)
    rules = output.get("rules")
    if not isinstance(rules, list):
        raise ValueError("compiler report must contain a rules list")
    sources = _source_rows(rows)
    changes: list[dict] = []
    for rule in rules:
        if not isinstance(rule, dict) or rule.get("status") == "baseline":
            continue
        matched = _matched_rows(rule, sources)
        if not matched:
            scope = str(rule.get("scope", workspace_scope))
            admitted = False
            reason = "unsupported-evidence-rescope"
        else:
            scope, admitted, reason = compiler._resolve_scope(
                requested=str(rule.get("scope", "")),
                record_type=str(rule.get("record_type", "")),
                rule=str(rule.get("rule", "")),
                matched_rows=matched,
                workspace_scope=workspace_scope,
                scope_catalog=scope_catalog,
                cited_evidence=[
                    str(item.get("evidence", ""))
                    for item in rule.get("support", [])
                    if isinstance(item, dict)
                ],
            )
        old_scope = rule.get("scope")
        old_status = rule.get("status")
        old_reason = rule.get("scope_reason")
        authority_admitted = rule.get("authority_reason") == "ok"
        rule["scope"] = scope
        rule["scope_reason"] = reason
        rule["status"] = "candidate" if admitted and authority_admitted else "quarantined"
        if (old_scope, old_status, old_reason) != (scope, rule["status"], reason):
            changes.append({
                "name": rule.get("name"),
                "old_scope": old_scope,
                "new_scope": scope,
                "old_status": old_status,
                "new_status": rule["status"],
                "old_reason": old_reason,
                "new_reason": reason,
            })
    status_counts = Counter(str(item.get("status")) for item in rules if isinstance(item, dict))
    scope_counts = Counter(str(item.get("scope")) for item in rules if isinstance(item, dict))
    output["rescope"] = {
        "algorithm": "deterministic-named-child-v1",
        "source_content_sha256": report.get("content_sha256"),
        "scope_catalog_sha256": compiler._hash_json(scope_catalog),
        "changed_rule_count": len(changes),
        "status_counts": dict(sorted(status_counts.items())),
        "scope_counts": dict(sorted(scope_counts.items())),
        "changes": changes,
    }
    output["rules_sha256"] = compiler._hash_json(rules)
    output.pop("content_sha256", None)
    output["content_sha256"] = compiler._hash_json(output)
    return output


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--results", type=Path, required=True)
    parser.add_argument("--commandcode-corpus", type=Path, required=True)
    parser.add_argument("--workspace-root", type=Path, default=Path("D:/Claude"))
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args(argv)

    workspace_scope = str(args.workspace_root.resolve()).replace(":", "-").replace("\\", "-").strip("-")
    groups, _ = compiler.load_commandcode_groups(
        args.commandcode_corpus, workspace_scope=workspace_scope
    )
    rows = [row for group in groups for row in group]
    scope_catalog = compiler.build_scope_catalog(args.workspace_root, rows)
    report = json.loads(args.results.read_text(encoding="utf-8"))
    expected_catalog_hash = report.get("preflight", {}).get("scope_catalog_sha256")
    actual_catalog_hash = compiler._hash_json(scope_catalog)
    if expected_catalog_hash and expected_catalog_hash != actual_catalog_hash:
        raise ValueError("scope catalog changed since compiler run")
    rescoped = rescope_report(
        report,
        rows=rows,
        scope_catalog=scope_catalog,
        workspace_scope=workspace_scope,
    )
    compiler._write_json(args.out, rescoped)
    print(json.dumps({
        "out": str(args.out),
        "rules_sha256": rescoped["rules_sha256"],
        **{key: rescoped["rescope"][key] for key in (
            "changed_rule_count", "status_counts", "scope_counts"
        )},
    }, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
