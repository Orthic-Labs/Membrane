"""Freeze the CommandCode Taste versus Adapt delivery-parity experiment.

The set is derived from the two compiler artifacts, not hand labels. It contains
shared, Taste-only, Adapt-only, and near-miss control cases. The builder also
emits an audited Adapt treatment manifest; source artifacts are never modified.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path


ROOT = next(p for p in Path(__file__).resolve().parents if (p / "tools" / "lib").is_dir())  # workspace root: the dir that owns tools/lib (never a fixed parent depth)
DEFAULT_TASTE = (
    ROOT / "docs/evidence/commandcode-taste-bakeoff-2026-07-13/"
    "minimax-m3/root-taste.md"
)
DEFAULT_ADAPT = (
    ROOT / ".cache/adapt-commandcode-delta-m3-seeded-sanitized-split1-v1/"
    "results-rescoped.json"
)
DEFAULT_OUT = ROOT / ".cache/adapt-delivery-parity/frozen"


EXCLUSIONS = {
    "taste-architecture-799c25df2a": "semantic duplicate of taste-architecture-7532d5593c",
    "taste-tooling-d69f97cfdf": "overlaps direct standing build-cancellation rule and is supported by process output",
    "taste-workflow-80a060511e": "one SEO task generalized into an overbroad standing policy",
    "taste-verification-a9f8072b55": "Playwright default conflicts with the workspace Tauri raw-CDP QA contract",
    "taste-tooling-722004f4d3": "Playwright sandbox workaround conflicts with the current QA/tool contract",
    "taste-tooling-12050d1654": "unsupported cmd-shim claim and conflicts with another candidate",
    "taste-tooling-be749f16ee": "cited evidence says the cmd path failed, so the rule is unsupported",
    "taste-model-routing-37fbe22444": "self-referential Adapt release rule unsupported by its cited evidence",
}


SHARED = [
    ("jsonl", 1, "taste-logging-9febf1852f", [
        "We need structured logs for this pipeline. Which serialization format should I use?",
        "Choose a format for machine-readable application logs and keep it consistent.",
    ]),
    ("persist-decisions", 3, "taste-workflow-50d323572b", [
        "We just locked an important project decision. Where should it be persisted?",
        "Make this architectural decision survive future Claude and Codex sessions.",
    ]),
    ("minimal-scope", 5, "taste-workflow-8cb99ce94b", [
        "I asked only for Python pill testing. Should we also add Rust and React settings?",
        "Keep this implementation tightly limited to the task I requested.",
    ]),
    ("actual-artifact", 6, "taste-workflow-70de2e417f", [
        "Export this model in a usable format so I can run it.",
        "The requested deliverable is a working technical artifact, not an explanation.",
    ]),
    ("brand-assets", 7, "taste-image-generation-c4b4519b21", [
        "Create image prompts for this brand. What must you inspect first?",
        "Derive the visual direction for a branded product shoot.",
    ]),
    ("validate-first", 13, "taste-workflow-790042b153", [
        "Validate this architecture plan for me before doing anything else.",
        "I asked for plan validation; what is the immediate next action?",
    ]),
    ("live-integration", 20, "taste-workflow-daed8ef8e3", [
        "The tool integration is wired and the binary exists. Is that enough to call it ready?",
        "What verification is required after configuring a new integration?",
    ]),
    ("production-rollback", 29, "taste-database-1fc8ed053f", [
        "Update these production database rows safely.",
        "What must happen before committing a production data correction?",
    ]),
]


TASTE_ONLY = [
    ("workflow-tabs", 15, [
        "We are reorganizing an AI media console. Should top tabs be Image/Video or named workflows such as KDP and Recraft?",
        "Several generation models have distinct operator workflows. How should the main navigation be grouped?",
    ]),
    ("fixed-presets", 16, [
        "A generation model supports one resolution and aspect ratio. Should its UI still expose every setting knob?",
        "Design the settings panel for a model whose technical requirements are fixed.",
    ]),
    ("minimax-default", 24, [
        "Choose the default execution model among the configured paid and free lanes.",
        "Which configured model should handle bulk implementation by default?",
    ]),
    ("avoid-cerebras", 26, [
        "We need a reliable provider for a review job. Should Cerebras be the default?",
        "Rank the configured free providers when stability matters most.",
    ]),
    ("vendored-git", 35, [
        "We are vendoring a fork into this repository. What should happen to the fork's .git directory?",
        "A copied upstream package appears as a nested repository. How should the host repo own it?",
    ]),
    ("active-app-layout", 40, [
        "Where should a current internal application live in this workspace?",
        "Should an actively developed tool be placed under Roadmap/Launches?",
    ]),
    ("mac-alongside-windows", 49, [
        "Drafting a Windows desktop architecture now: when should macOS ARM be addressed?",
        "A new Tauri app is only built on Windows today. How should platform support enter the design docs?",
    ]),
    ("checkout-subpath", 52, [
        "For an app domain, should shared checkout use a store subdomain or a /checkout route?",
        "How should a shared storefront be exposed without creating store.app subdomains?",
    ]),
]


ADAPT_ONLY = [
    ("cancel-prior-build", "taste-workflow-af3b148e41", [
        "I am starting another build while a previous cargo or esbuild process may still be active. What comes first?",
        "The machine is overloaded by overlapping builds. What workflow prevents that on the next cycle?",
    ]),
    ("headless-chrome", "taste-code-style-cdfe500fa6", [
        "Capture mobile and desktop screenshots from a local site. Which Chrome profile and scale flags keep them stable?",
        "Headless Chrome has a profile lock and the mobile screenshot width is wrong. How should it be invoked?",
    ]),
    ("accurate-hero", "taste-workflow-3b66fdc0c9", [
        "A generated hero image does not resemble the actual application. What should replace it?",
        "The marketing screenshot asset is inaccurate. What visual implementation and verification loop should we use?",
    ]),
    ("sellright-cutover", "taste-architecture-4fd76b0fde", [
        "We are preparing the Damned Designs commerce cutover from Vendure. What rollback and parity safeguards are required?",
        "A new SellRight provider seam is going live. What must remain available until totals are verified?",
    ]),
    ("ssh-config-safety", "taste-safety-89eb40017e", [
        "I need non-secret host and port values from a remote config, but my grep pattern also includes a token. How should I proceed?",
        "An SSH diagnostic needs selected config values without exposing credentials. How should the read be scoped?",
    ]),
    ("vast-hashes", "taste-verification-bd8b85b4ff", [
        "A GPU evaluation was copied back and I may destroy the instance. What verification should happen first?",
        "How do I confirm the local mirror matches the important remote evaluation artifacts before teardown?",
    ]),
    ("heartbeat", "taste-workflow-0b79d46e82", [
        "A long Windows training run may die while nobody is watching. How should it be monitored?",
        "How can we detect a detached evaluation failure without waiting for the user to ask for status?",
    ]),
    ("wsl-elevation", "taste-tooling-71c713b5e1", [
        "A WSL distro was installed from an elevated shell, but the normal shell lists no distros. Which context should manage it?",
        "Codex cannot see the named WSL distro after an administrator installed it. How should commands be run?",
    ]),
]


CONTROLS = [
    ("csv-export", "Export these records as CSV for an accountant.", ["jsonl"]),
    ("test-db", "Update three rows in a disposable local test database fixture.", ["production-rollback"]),
    ("image-grid", "Lay out an image grid on a normal responsive website page.", ["fixed-presets"]),
    ("explicit-model", "Use the explicitly requested GLM model for this one comparison.", ["minimax-default"]),
    ("localhost-route", "Add a localhost-only debug route to this development server.", ["checkout-subpath"]),
    ("api-unit-test", "Write a pure unit test for this JSON parser with no browser involved.", ["headless-chrome"]),
    ("unrelated-process", "A separate signed release build is running. Start the requested lint check without disturbing it.", ["cancel-prior-build"]),
    ("rightapps-docs", "Clarify a RightApps documentation paragraph; no commerce cutover is involved.", ["sellright-cutover"]),
    ("native-windows", "Inspect a native Windows registry value; WSL is not involved.", ["wsl-elevation"]),
    ("local-review", "Review this local two-line diff yourself; no external reviewer was requested.", ["live-integration"]),
    ("generic-css", "Change the padding on an existing accurate hero section.", ["accurate-hero"]),
    ("mac-only", "Build a tiny macOS-only menu-bar utility with no planned Windows release.", ["mac-alongside-windows"]),
]


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def parse_taste(path: Path) -> list[dict]:
    category = "uncategorized"
    rules: list[dict] = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if line.startswith("# ") and not line.startswith("# Taste"):
            category = line[2:].strip()
        match = re.fullmatch(r"- (.+?) Confidence: ([0-9.]+)", line)
        if match:
            rules.append({
                "index": len(rules) + 1,
                "category": category,
                "rule": match.group(1).strip(),
                "confidence": float(match.group(2)),
            })
    return rules


def build_adapt_treatment(path: Path) -> dict:
    source = json.loads(path.read_text(encoding="utf-8"))
    source_rules = source.get("rules", [])
    by_name = {r["name"]: r for r in source_rules}
    missing = sorted(set(EXCLUSIONS) - set(by_name))
    if missing:
        raise ValueError(f"exclusion ids missing from Adapt artifact: {missing}")
    records = []
    for item in source_rules:
        if item.get("status") not in {"baseline", "candidate"}:
            continue
        record = dict(item)
        record["id"] = record["name"]
        record["source_status"] = record["status"]
        record["status"] = "accepted"
        records.append(record)
    if len(records) != 57:
        raise ValueError(f"expected 57 raw usable Adapt records, found {len(records)}")
    curated_records = [r for r in records if r["id"] not in EXCLUSIONS]
    if len(curated_records) != 49:
        raise ValueError(f"expected 49 secondary curated records, found {len(curated_records)}")
    return {
        "schema_version": 1,
        "primary_treatment": "raw-usable-output",
        "source_path": str(path),
        "source_sha256": _sha256(path),
        "records": records,
        "records_sha256": hashlib.sha256(json.dumps(
            records, sort_keys=True, ensure_ascii=False).encode("utf-8")).hexdigest(),
        "curated_records": curated_records,
        "exclusions": [
            {"id": key, "reason": reason, "source_rule": by_name[key]["rule"]}
            for key, reason in EXCLUSIONS.items()
        ],
    }


def _case(case_id: str, cohort: str, concept: str, prompt: str, variant: int,
          expected: dict | None, scope: str = "D--Claude",
          forbidden: list[str] | None = None) -> dict:
    return {
        "case_id": case_id,
        "cohort": cohort,
        "concept": concept,
        "variant": variant,
        "prompt": prompt,
        "scope": scope,
        "expected": expected,
        "forbidden_concepts": forbidden or [],
    }


def build_value_set(taste_path: Path, adapt_path: Path) -> tuple[dict, dict]:
    taste = parse_taste(taste_path)
    if len(taste) != 52:
        raise ValueError(f"expected 52 CommandCode root rules, found {len(taste)}")
    taste_by_index = {r["index"]: r for r in taste}
    treatment = build_adapt_treatment(adapt_path)
    adapt_by_id = {r["id"]: r for r in treatment["records"]}
    rows: list[dict] = []

    for concept, index, adapt_id, prompts in SHARED:
        tr, ar = taste_by_index[index], adapt_by_id[adapt_id]
        expected = {
            "taste_rule_index": index, "taste_rule": tr["rule"],
            "adapt_id": adapt_id, "adapt_rule": ar["rule"],
        }
        for variant, prompt in enumerate(prompts, 1):
            rows.append(_case(f"shared-{concept}-{variant}", "shared", concept,
                              prompt, variant, expected, ar.get("scope", "D--Claude")))

    for concept, index, prompts in TASTE_ONLY:
        tr = taste_by_index[index]
        expected = {"taste_rule_index": index, "taste_rule": tr["rule"]}
        for variant, prompt in enumerate(prompts, 1):
            rows.append(_case(f"taste-{concept}-{variant}", "taste_only", concept,
                              prompt, variant, expected))

    for concept, adapt_id, prompts in ADAPT_ONLY:
        ar = adapt_by_id[adapt_id]
        expected = {"adapt_id": adapt_id, "adapt_rule": ar["rule"]}
        for variant, prompt in enumerate(prompts, 1):
            rows.append(_case(f"adapt-{concept}-{variant}", "adapt_only", concept,
                              prompt, variant, expected, ar.get("scope", "D--Claude")))

    for concept, prompt, forbidden in CONTROLS:
        rows.append(_case(f"control-{concept}", "control", concept, prompt, 1,
                          None, forbidden=forbidden))

    if len(rows) != 60 or len({r["case_id"] for r in rows}) != 60:
        raise ValueError("parity value set must contain 60 unique cases")
    lowered_prompts = "\n".join(r["prompt"].casefold() for r in rows)
    for row in rows:
        expected = row.get("expected") or {}
        for key in ("taste_rule", "adapt_rule"):
            rule = expected.get(key, "").casefold()
            if rule and rule in lowered_prompts:
                raise ValueError(f"expected-rule leakage in prompt set: {row['case_id']}")

    payload = {
        "schema_version": 1,
        "design": "five-arm-delivery-parity-v1",
        "sources": {
            "taste_path": str(taste_path), "taste_sha256": _sha256(taste_path),
            "adapt_path": str(adapt_path), "adapt_sha256": _sha256(adapt_path),
        },
        "counts": {"total": 60, "shared": 16, "taste_only": 16,
                   "adapt_only": 16, "control": 12},
        "taste_rules": taste,
        "cases": rows,
    }
    encoded = json.dumps(payload, sort_keys=True, ensure_ascii=False).encode("utf-8")
    payload["content_sha256"] = hashlib.sha256(encoded).hexdigest()
    return payload, treatment


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--taste", type=Path, default=DEFAULT_TASTE)
    parser.add_argument("--adapt", type=Path, default=DEFAULT_ADAPT)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    args = parser.parse_args(argv)
    value_set, treatment = build_value_set(args.taste, args.adapt)
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "value-set.json").write_text(
        json.dumps(value_set, indent=2, ensure_ascii=False), encoding="utf-8")
    (args.out / "adapt-treatment.json").write_text(
        json.dumps(treatment, indent=2, ensure_ascii=False), encoding="utf-8")
    print(f"frozen: {len(value_set['cases'])} cases")
    print(f"taste: {len(value_set['taste_rules'])} rules")
    print(f"adapt primary: {len(treatment['records'])} raw usable records")
    print(f"adapt secondary: {len(treatment['curated_records'])} curated records; "
          f"flagged={len(treatment['exclusions'])}")
    print(args.out / "value-set.json")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
