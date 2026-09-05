#!/usr/bin/env python3
"""Merge the reviewed current main into ledger-end-to-end and resolve two known conflicts.

This script is temporary branch integration machinery. It refuses main drift,
unexpected conflict paths, conflict-shape drift, and force pushing. It runs no
builds, tests, binaries, installation, activation, or release actions.
"""
from pathlib import Path
import os
import subprocess

ROOT = Path(__file__).resolve().parents[2]
BRANCH = "ledger-end-to-end"
EXPECTED_MAIN = "4e563fc3f43ed3d05e7f04b7c655852478479fb3"
EXPECTED_CONFLICTS = {
    "engine/crates/membrane-mcp/src/tools.rs",
    "engine/crates/membrane-runtime/src/mcp_executor.rs",
}


def run(*args, check=True):
    return subprocess.run(args, cwd=ROOT, text=True, capture_output=True, check=check)


def replace_once(text: str, before: str, after: str, label: str) -> str:
    count = text.count(before)
    if count != 1:
        raise SystemExit(f"conflict-shape drift in {label}: expected one match, got {count}")
    return text.replace(before, after, 1)


def resolve_tools(path: Path):
    text = path.read_text(encoding="utf-8")
    text = replace_once(text,
'''<<<<<<< HEAD
    "membrane_ledger",
=======
    "membrane_push_prepare",
    "membrane_push_resolve",
>>>>>>> origin/main''',
'''    "membrane_ledger",
    "membrane_push_prepare",
    "membrane_push_resolve",''', "tools CORE")
    text = replace_once(text,
'''<<<<<<< HEAD
    let (required, mut properties) = match name {
=======
    if name.starts_with("membrane_push_") {
        let definitions: Value = serde_json::from_str(include_str!("../../../../schemas/registry/push-tools.v1.json")).expect("Push schemas parse");
        return definitions.as_array().unwrap().iter().find(|v| v["name"] == name).expect("Push tool registered")["inputSchema"].clone();
    }
    let (required, properties) = match name {
>>>>>>> origin/main''',
'''    if name.starts_with("membrane_push_") {
        let definitions: Value = serde_json::from_str(include_str!("../../../../schemas/registry/push-tools.v1.json")).expect("Push schemas parse");
        return definitions.as_array().unwrap().iter().find(|v| v["name"] == name).expect("Push tool registered")["inputSchema"].clone();
    }
    let (required, mut properties) = match name {''', "tools schema dispatch")
    text = replace_once(text,
'''<<<<<<< HEAD
            json!({"task":{"type":"string","minLength":1,"pattern":"\\\\S"},"repository":{"type":"string"},"caller":caller(),"budget":{"type":"integer","minimum":1},"scope":{"type":"string","enum":["repo","workspace"]},"taskId":{"type":"string","minLength":1},"deadlineMs":{"type":"integer","minimum":1},"sufficiencyContract":{"type":"object","description":"Optional planner-authored SufficiencyContractV1 (membrane-sufficiency-v1); transported verbatim to federate, never derived from task prose"},"remainingContextCeiling":remaining_context_ceiling()}),
=======
            json!({"task":{"type":"string","minLength":1,"pattern":"\\\\S"},"repository":{"type":"string"},"caller":caller(),"budget":{"type":"integer","minimum":1},"scope":{"type":"string","enum":["repo","workspace"]},"deadlineMs":{"type":"integer","minimum":1},"sufficiencyContract":{"type":"object","description":"Optional planner-authored SufficiencyContractV1 (membrane-sufficiency-v1); transported verbatim to federate, never derived from task prose"},"remainingContextCeiling":remaining_context_ceiling(),"pushResolverToken":{"type":"string","minLength":64,"maxLength":64}}),
>>>>>>> origin/main''',
'''            json!({"task":{"type":"string","minLength":1,"pattern":"\\\\S"},"repository":{"type":"string"},"caller":caller(),"budget":{"type":"integer","minimum":1},"scope":{"type":"string","enum":["repo","workspace"]},"taskId":{"type":"string","minLength":1},"deadlineMs":{"type":"integer","minimum":1},"sufficiencyContract":{"type":"object","description":"Optional planner-authored SufficiencyContractV1 (membrane-sufficiency-v1); transported verbatim to federate, never derived from task prose"},"remainingContextCeiling":remaining_context_ceiling(),"pushResolverToken":{"type":"string","minLength":64,"maxLength":64}}),''', "context schema")
    text = replace_once(text,
'''<<<<<<< HEAD
        if !matches!(group, "default" | "memory" | "blueprint" | "diagnostic" | "ledger")
=======
        if !matches!(group, "default" | "memory" | "blueprint" | "diagnostic" | "push")
>>>>>>> origin/main''',
'''        if !matches!(group, "default" | "memory" | "blueprint" | "diagnostic" | "ledger" | "push")''', "toolsets")
    text = replace_once(text,
'''<<<<<<< HEAD
            "ledger" => &["membrane_source_read", "membrane_ledger"],
=======
            "push" => &CORE[10..],
>>>>>>> origin/main''',
'''            "ledger" => &["membrane_source_read", "membrane_ledger"],
            "push" => &CORE[11..],''', "toolset slices")
    path.write_text(text, encoding="utf-8")


def resolve_executor(path: Path):
    text = path.read_text(encoding="utf-8")
    text = replace_once(text,
'''<<<<<<< HEAD
        "membrane_source_read" => "source_read",
        "membrane_ledger" => match arguments.get("operation").and_then(Value::as_str) {
            Some("erase" | "activate") => "checkpoint",
            Some("status") => "system_status",
            _ => "context",
        },
=======
        "membrane_source_read" | "membrane_push_prepare" | "membrane_push_resolve" => "source_read",
>>>>>>> origin/main''',
'''        "membrane_source_read" | "membrane_push_prepare" | "membrane_push_resolve" => "source_read",
        "membrane_ledger" => match arguments.get("operation").and_then(Value::as_str) {
            Some("erase" | "activate") => "checkpoint",
            Some("status") => "system_status",
            _ => "context",
        },''', "native action")

    marker = '<<<<<<< HEAD\n            "membrane_source_read" | "membrane_ledger" => {'
    start = text.find(marker)
    if start < 0:
        raise SystemExit("source-read conflict start drifted")
    middle = text.find("=======\n", start)
    end_marker = ">>>>>>> origin/main"
    end = text.find(end_marker, middle)
    if middle < 0 or end < 0:
        raise SystemExit("source-read conflict markers drifted")
    # Keep Ledger's shared daemon-owner prelude. The tail after the conflict is
    # already the Ledger owner's bounded dispatch; current main's Push context
    # delivery and push-tool dispatch remain outside this conflict and are kept.
    ours = text[start + len("<<<<<<< HEAD\n"):middle].rstrip("\n")
    text = text[:start] + ours + text[end + len(end_marker):]
    path.write_text(text, encoding="utf-8")


def main():
    if os.environ.get("GITHUB_REF") != f"refs/heads/{BRANCH}":
        raise SystemExit("refused: wrong branch")
    run("git", "fetch", "origin", "main")
    observed = run("git", "rev-parse", "origin/main").stdout.strip()
    if observed != EXPECTED_MAIN:
        raise SystemExit(f"refused: main moved from {EXPECTED_MAIN} to {observed}")
    merge = run("git", "merge", "--no-commit", "--no-ff", "origin/main", check=False)
    if merge.returncode not in (0, 1):
        raise SystemExit(merge.stderr or merge.stdout)
    conflicts = set(filter(None, run("git", "diff", "--name-only", "--diff-filter=U").stdout.splitlines()))
    if conflicts != EXPECTED_CONFLICTS:
        raise SystemExit(f"unexpected merge conflicts: {sorted(conflicts)}")

    resolve_tools(ROOT / "engine/crates/membrane-mcp/src/tools.rs")
    resolve_executor(ROOT / "engine/crates/membrane-runtime/src/mcp_executor.rs")
    for relative in EXPECTED_CONFLICTS:
        text = (ROOT / relative).read_text(encoding="utf-8")
        if any(marker in text for marker in ("<<<<<<<", "=======", ">>>>>>>")):
            raise SystemExit(f"unresolved marker in {relative}")
        run("git", "add", "--", relative)
    if run("git", "diff", "--name-only", "--diff-filter=U").stdout.strip():
        raise SystemExit("unresolved paths remain")

    # Remove the preview-only workflow and this integration helper from the
    # merged tree; neither is product/runtime code.
    for relative in [".github/workflows/ledger-merge-preview.yml", "scripts/ci/ledger-merge-main.py"]:
        path = ROOT / relative
        if path.exists():
            run("git", "rm", "--", relative)

    run("git", "diff", "--check")
    run("git", "config", "user.name", "github-actions[bot]")
    run("git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com")
    run("git", "commit", "-m", "merge(ledger): integrate current main without losing Push or Blueprint\n\nResolve the only two overlapping surfaces by composition: Ledger remains discoverable and daemon-owned, while current Push schemas, resolver tokens, delivery fitting and tool dispatch remain intact. No builds, tests, activation, or release performed.\n\n[skip ci]")
    run("git", "push", "origin", f"HEAD:refs/heads/{BRANCH}")
    print(run("git", "rev-parse", "HEAD").stdout.strip())

if __name__ == "__main__":
    main()
