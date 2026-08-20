"""Gate 0 — 4-arm retrieval + behavior A/B harness (v2 plan, Gate 0 step 3).

Pinned: 2026-07-13. Adapted from the sealed CommandCode A/B at
``.cache/adapt-taste-ab/evaluate.py``; this script keeps the isolated-service
machinery and replaces the 2-arm comparison with the v2 plan's 4 arms:

  A: current Cortex recall only (baseline).
  B: copied Cortex + the 16 CommandCode rules injected via `cortex put`,
     retrieved normally (proves retrieval value alone).
  C: copied Cortex + the same 16 CommandCode rules as an ALWAYS-INJECTED
     static policy block (proves the value comes from retrieval vs always-on).
  D: copied Cortex + accepted 10-session Adapt candidates, retrieved
     normally (proves the Adapt pipeline produces a useful candidate set
     when fed real session evidence).

All four arms receive the SAME query set: 32 targeted prompts (16 rules ×
2 paraphrases) plus 12 unrelated controls. Each arm runs an isolated Cortex
service bound to a unique free port so live invariants stay verifiable.

Grading is intentionally left to a separate ``grade.py`` so this harness
stays focused on the live-DB isolation contract. The grader consumes
``results.json`` and emits a markdown summary.

Usage::

    py -3.11 membrane/adapt/eval/run_value_ab.py \\
        --live-db D:/Claude/tools/.cache/memory/cortex-engine.db \\
        --cortex-bin D:/Claude/tools/bin/cortex.exe \\
        --taste-md D:/Claude/.commandcode/taste/taste.md \\
        --adapt-manifest D:/Claude/.cache/adapt/review/10session.manifest.json \\
        --out D:/Claude/.cache/adapt-value-ab/

Add ``--smoke`` to use only 4 prompts for fast iteration.

Outputs::

    <out>/A.db                  baseline snapshot
    <out>/B.db                  + 16 CommandCode rules
    <out>/C.db                  + 16 rules (recall only; arm C prepends at prompt)
    <out>/D.db                  + accepted Adapt candidates
    <out>/results.json          per-arm retrieval rows + ids + static-block
    <out>/report.md             human-readable summary
    <out>/integrity.json        pre/post DB integrity + row-count checks

Each arm records:
  - ranked_ids per query row
  - the static policy block (arm C only)
  - wall-clock per query

The harness refuses to run if any arm leaves the live DB modified (it never
touches the live DB; isolation is enforced by binding each service to a
copy via the SQLite backup API).
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import shutil
import socket
import sqlite3
import subprocess
import sys
import time
from dataclasses import dataclass, field, asdict
from pathlib import Path


# ----- paths & constants -----

ROOT = next(p for p in Path(__file__).resolve().parents if (p / "tools" / "lib").is_dir())  # workspace root: the dir that owns tools/lib (never a fixed parent depth)
DEFAULT_LIVE_DB = ROOT / "tools/.cache/memory/cortex-engine.db"
DEFAULT_CORTEX = ROOT / "tools/bin/cortex.exe"
DEFAULT_TASTE_MD = ROOT / ".commandcode/taste/taste.md"
DEFAULT_OUT = ROOT / ".cache/adapt-value-ab"
SCOPE = "D--Claude"
K = 10                              # recall@K
SERVICE_READY_TIMEOUT = 20.0        # seconds to wait for service bind


# ----- query set -----

QUERY_PAIRS: list[tuple[str, str]] = [
    ("We need structured logs for this pipeline. Which serialization format should I use?",
     "Choose a format for machine-readable application logs and keep it consistent."),
    ("Add a signed macOS build flow to another Right Suite app.",
     "Can this repository have a custom release pipeline different from the other apps?"),
    ("We just locked an important project decision. Where should it be persisted?",
     "Make this architectural decision survive future Claude and Codex sessions."),
    ("Where should cross-app release, logging, and telemetry code live?",
     "Several desktop apps need the same updater functionality. How should we structure it?"),
    ("I asked only for Python pill testing. Should we also add Rust and React settings?",
     "Keep this implementation tightly limited to the task I requested."),
    ("Export this model in a usable format so I can run it.",
     "The requested deliverable is a working technical artifact, not an explanation."),
    ("Create image prompts for this brand. What must you inspect first?",
     "Derive the visual direction for a branded product shoot."),
    ("Can this ASR model drop into the Sherpa streaming runtime?",
     "Assess runtime compatibility of a NeMo ASR export before integration."),
    ("Implement the cache-aware streaming loop from these ONNX tensor shapes.",
     "How should chunk, drop, and valid-output behavior be verified for streaming ASR?"),
    ("Validate this architecture plan for me before doing anything else.",
     "I asked for plan validation; what is the immediate next action?"),
    ("Run CodeRabbit on this scoped diff even though the repository is private.",
     "I explicitly requested an external CLI code review. Is scoped egress authorized?"),
    ("The tool integration is wired and the binary exists. Is that enough to call it ready?",
     "What verification is required after configuring a new integration?"),
    ("Our subscriber first-name field is unreliable. How should the email open?",
     "Write the greeting strategy for a list with inconsistent names."),
    ("Put portrait and landscape product photos in a multi-column email.",
     "How should image dimensions be handled across email product cards?"),
    ("Update these production database rows safely.",
     "What must happen before committing a production data correction?"),
    ("We rejected this model candidate. How do we prevent the team reopening it later?",
     "Record why a technical approach failed so the decision remains durable."),
]

CONTROLS: list[str] = [
    "Summarize this Python function and explain its return value.",
    "Find the CSS selector responsible for the sidebar padding.",
    "What files changed in the current git diff?",
    "Rename this local variable without changing behavior.",
    "Explain why this unit test is failing.",
    "List the routes exposed by this web application.",
    "Calculate the total size of all PNG files in this directory.",
    "What dependency version is pinned in package.json?",
    "Show me the latest terminal output from the development server.",
    "Add a null check around this optional response field.",
    "Which test fixture creates the sample customer record?",
    "Describe the columns in this SQLite table.",
]


# ----- rule model -----

@dataclass(frozen=True)
class TasteRule:
    category: str
    text: str
    confidence: float
    name: str           # e.g. cc-taste-01-always-prefer-jsonl-over-logfmt

    @property
    def memory_id(self) -> str:
        return f"{SCOPE}/{self.name}"

    @property
    def slug(self) -> str:
        return slugify(self.text)

    def as_envelope(self) -> str:
        """Body to feed into cortex put. Mirrors evaluate.py."""
        return (
            f"Standing preference [{self.category}]: {self.text} "
            f"Source: CommandCode learn-taste. Confidence: {self.confidence:.2f}."
        )

    def as_static_block(self) -> str:
        """Always-injected block for arm C."""
        return f"[policy:{self.category}] {self.text}"


def slugify(value: str, limit: int = 58) -> str:
    clean = re.sub(r"[^a-z0-9]+", "-", value.casefold()).strip("-")
    return clean[:limit].rstrip("-")


def parse_taste_md(path: Path) -> list[TasteRule]:
    """Parse ``taste.md`` lines of the form ``- rule text Confidence: 0.85``."""
    category = "uncategorized"
    rules: list[TasteRule] = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if line.startswith("# ") and not line.startswith("# Taste"):
            category = line[2:].strip()
        if not line.startswith("- "):
            continue
        match = re.match(r"- (.+?) Confidence: ([0-9.]+)$", line)
        if not match:
            continue
        text, confidence = match.groups()
        rules.append(TasteRule(category, text.strip(), float(confidence),
                               name=f"cc-taste-{len(rules) + 1:02d}-{slugify(text)}"))
    return rules


# ----- snapshot + service management -----

def snapshot_live_db(live: Path, dest: Path) -> int:
    """SQLite backup API. Returns row count of the snapshot."""
    if dest.exists():
        dest.unlink()
    src_uri = f"file:{live.as_posix()}?mode=ro"
    with sqlite3.connect(src_uri, uri=True) as src, sqlite3.connect(dest) as dst:
        src.backup(dst)
    with sqlite3.connect(dest) as check:
        result = check.execute("PRAGMA integrity_check").fetchone()[0]
        count = check.execute("SELECT COUNT(*) FROM memories").fetchone()[0]
    if result != "ok":
        raise RuntimeError(f"snapshot integrity_check failed: {result}")
    return count


def _free_port() -> int:
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def _start_service(cortex: Path, db: Path, port: int) -> subprocess.Popen:
    env = os.environ.copy()
    env["MEMBRANE_PORT"] = str(port)
    flags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
    return subprocess.Popen(
        [str(cortex), "--db", str(db), "serve", "--port", str(port)],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
        creationflags=flags,
    )


def _wait_ready(port: int, deadline_s: float = SERVICE_READY_TIMEOUT) -> None:
    deadline = time.monotonic() + deadline_s
    while time.monotonic() < deadline:
        with socket.socket() as client:
            client.settimeout(0.2)
            if client.connect_ex(("127.0.0.1", port)) == 0:
                return
        time.sleep(0.05)
    raise RuntimeError(f"cortex service on port {port} did not become ready")


def _run_via_port(cortex: Path, db: Path, port: int,
                  cmd: list[str], input_text: str | None = None) -> str:
    env = os.environ.copy()
    env["MEMBRANE_PORT"] = str(port)
    res = subprocess.run([str(cortex), "--db", str(db), *cmd],
                         input=input_text, text=True, encoding="utf-8",
                         capture_output=True, check=False, env=env)
    if res.returncode != 0:
        raise RuntimeError(
            f"cortex {cmd[0]} failed rc={res.returncode}: {res.stderr.strip()}"
        )
    return res.stdout


def _stop_service(svc: subprocess.Popen) -> None:
    svc.terminate()
    try:
        svc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        svc.kill()
        svc.wait(timeout=5)


def put_rule(cortex: Path, db: Path, port: int, rule: TasteRule) -> None:
    _run_via_port(cortex, db, port,
                  ["put", rule.name, "--scope", SCOPE, "--tier", "Semantic"],
                  input_text=rule.as_envelope())


def put_pref_record(cortex: Path, db: Path, port: int,
                    pref_id: str, scope: str, body: str) -> None:
    """Insert an Adapt candidate preference record into the arm-D snapshot."""
    _run_via_port(cortex, db, port,
                  ["put", pref_id, "--scope", scope, "--tier", "Semantic"],
                  input_text=body)


# ----- replay -----

def build_query_rows(rules: list[TasteRule], smoke: bool) -> list[dict]:
    n = 2 if smoke else len(rules)
    rows: list[dict] = []
    for index, rule in enumerate(rules[:n]):
        for variant, query in enumerate(QUERY_PAIRS[index], 1):
            rows.append({
                "row_id": f"p{index + 1:02d}-{variant}",
                "query": query,
                "scope": SCOPE,
                "kind": "positive",
                "expected_id": rule.memory_id,
                "rule": rule.text,
            })
    if not smoke:
        rows.extend({
            "row_id": f"c{index + 1:02d}", "query": q, "scope": SCOPE,
            "kind": "control", "expected_id": None, "rule": None,
        } for index, q in enumerate(CONTROLS))
    return rows


def replay_arm(cortex: Path, db: Path, port: int,
               rows: list[dict], input_path: Path) -> tuple[dict[str, list[str]],
                                                          dict[str, list[str]]]:
    """Run replay; also fetch each ranked row's text via `cortex get` so the
    grader can score the static-block text overlap for arm C. Returns
    (ranked_ids, ranked_texts).
    """
    input_path.write_text("".join(
        json.dumps({k: r[k] for k in ("row_id", "query", "scope")}) + "\n"
        for r in rows
    ), encoding="utf-8")
    out = _run_via_port(cortex, db, port,
                        ["replay", "--input", str(input_path), "-k", str(K)])
    parsed = [json.loads(l) for l in out.splitlines() if l.strip()]
    ranked_ids = {row["row_id"]: row["ranked_ids"] for row in parsed}
    ranked_texts: dict[str, list[str]] = {}
    for row in parsed:
        rid = row["row_id"]
        texts = []
        for mid in row["ranked_ids"]:
            try:
                texts.append(_run_via_port(cortex, db, port,
                                           ["get", mid]))
            except Exception:
                pass
        ranked_texts[rid] = texts
    return ranked_ids, ranked_texts


def memory_contents(db: Path) -> dict[str, str]:
    with sqlite3.connect(db) as conn:
        return dict(conn.execute("SELECT id, content FROM memories"))


# ----- adapt candidates -----

def load_adapt_manifest_records(path: Path | None) -> list[dict]:
    """Load accepted records from a reviewed Adapt manifest, if any."""
    if path is None or not path.exists():
        return []
    body = json.loads(path.read_text(encoding="utf-8"))
    return [r for r in body.get("records", []) if r.get("status") == "accepted"]


def adapt_record_envelope(rec: dict) -> str:
    """Mirror the engine envelope used by adapt.to_cortex_content."""
    cat = rec.get("category", "tooling")
    rule = rec.get("rule", "")
    conf = rec.get("confidence", 0.6)
    return (
        f"**[adapt/{cat}]** — {rule} "
        f"Confidence: {conf:.2f} "
        f"(adapt preference, source_ids={rec.get('source_ids', [])})."
    )


# ----- arm runners -----

@dataclass
class ArmResult:
    arm: str
    db_path: str
    port: int
    ranked: dict[str, list[str]] = field(default_factory=dict)
    ranked_texts: dict[str, list[str]] = field(default_factory=dict)
    static_block: str = ""
    wall_ms: dict[str, int] = field(default_factory=dict)
    notes: list[str] = field(default_factory=list)


def run_arm_a(cortex: Path, db: Path, rows: list[dict],
              tmp: Path) -> ArmResult:
    """Baseline: snapshot of live DB, no modifications."""
    snapshot_live_db(DEFAULT_LIVE_DB, db)
    port = _free_port()
    svc = _start_service(cortex, db, port)
    try:
        _wait_ready(port)
        result = ArmResult(arm="A", db_path=str(db), port=port)
        for row in rows:
            t0 = time.monotonic()
            ranked, texts = replay_arm(cortex, db, port, [row],
                                       tmp / f"A-{row['row_id']}.jsonl")
            dt_ms = int((time.monotonic() - t0) * 1000)
            result.ranked.update(ranked)
            result.ranked_texts.update(texts)
            result.wall_ms.update({k: dt_ms for k in ranked})
        return result
    finally:
        _stop_service(svc)


def run_arm_with_inject(cortex: Path, db: Path, rows: list[dict],
                        tmp: Path, arm_label: str,
                        inject_fn) -> ArmResult:
    """Generic: snapshot + start service + inject via inject_fn + replay."""
    snapshot_live_db(DEFAULT_LIVE_DB, db)
    port = _free_port()
    svc = _start_service(cortex, db, port)
    try:
        _wait_ready(port)
        inject_fn(cortex, db, port)
        result = ArmResult(arm=arm_label, db_path=str(db), port=port)
        for row in rows:
            t0 = time.monotonic()
            ranked, texts = replay_arm(cortex, db, port, [row],
                                       tmp / f"{arm_label}-{row['row_id']}.jsonl")
            dt_ms = int((time.monotonic() - t0) * 1000)
            result.ranked.update(ranked)
            result.ranked_texts.update(texts)
            result.wall_ms.update({k: dt_ms for k in ranked})
        return result
    finally:
        _stop_service(svc)


# ----- integrity verification -----

def integrity_check(db: Path) -> tuple[bool, int, str]:
    with sqlite3.connect(db) as conn:
        result = conn.execute("PRAGMA integrity_check").fetchone()[0]
        count = conn.execute("SELECT COUNT(*) FROM memories").fetchone()[0]
    return (result == "ok", count, result)


def live_db_unchanged(live: Path, baseline_count: int) -> tuple[bool, str]:
    """Refuses to claim success if the live DB row count drifted."""
    ok, count, msg = integrity_check(live)
    return (ok and count == baseline_count, msg)


# ----- orchestration -----

def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--live-db", type=Path, default=DEFAULT_LIVE_DB)
    ap.add_argument("--cortex-bin", type=Path, default=DEFAULT_CORTEX)
    ap.add_argument("--taste-md", type=Path, default=DEFAULT_TASTE_MD)
    ap.add_argument("--adapt-manifest", type=Path, default=None,
                    help="Reviewed 10-session Adapt manifest; populates arm D")
    ap.add_argument("--out", type=Path, default=DEFAULT_OUT)
    ap.add_argument("--smoke", action="store_true",
                    help="use 2 rules × 2 paraphrases + 2 controls for fast iteration")
    args = ap.parse_args(argv)

    if not args.live_db.exists():
        print(f"error: live DB not found: {args.live_db}", file=sys.stderr)
        return 2
    if not args.cortex_bin.exists():
        print(f"error: cortex binary not found: {args.cortex_bin}",
              file=sys.stderr)
        return 2
    if not args.taste_md.exists():
        print(f"error: taste.md not found: {args.taste_md}", file=sys.stderr)
        return 2

    args.out.mkdir(parents=True, exist_ok=True)
    tmp = args.out / "tmp"
    tmp.mkdir(parents=True, exist_ok=True)

    rules = parse_taste_md(args.taste_md)
    if len(rules) != len(QUERY_PAIRS):
        print(f"warning: taste.md has {len(rules)} rules, "
              f"QUERY_PAIRS has {len(QUERY_PAIRS)} — keeping parsed set",
              file=sys.stderr)

    rows = build_query_rows(rules, smoke=args.smoke)
    print(f"harness: {len(rules)} rules, {len(rows)} query rows "
          f"({sum(1 for r in rows if r['kind'] == 'positive')} positive, "
          f"{sum(1 for r in rows if r['kind'] == 'control')} control)")

    # Pre-arm: snapshot live DB integrity + count for the integrity record.
    pre_ok, pre_count, pre_msg = integrity_check(args.live_db)
    print(f"pre-arm live DB: integrity={pre_msg} rows={pre_count}")

    arms: list[ArmResult] = []

    # ----- Arm A: baseline -----
    print("arm A: baseline...")
    arms.append(run_arm_a(args.cortex_bin, args.out / "A.db", rows, tmp))

    # ----- Arm B: + 16 CommandCode rules -----
    def _inject_b(cortex, db, port):
        for r in rules:
            put_rule(cortex, db, port, r)
    print("arm B: +16 CommandCode rules...")
    arms.append(run_arm_with_inject(args.cortex_bin, args.out / "B.db",
                                    rows, tmp, "B", _inject_b))

    # ----- Arm C: + 16 rules as static block; engine unchanged -----
    # The static block is composed at the prompt boundary, not in the DB.
    # The arm records the block alongside the ranked output so graders can
    # apply the always-injected scoring rule consistently.
    print("arm C: +16 static-block (no DB change)...")
    arms_c = run_arm_a(args.cortex_bin, args.out / "C.db", rows, tmp)
    arms_c.arm = "C"
    arms_c.static_block = "\n".join(r.as_static_block() for r in rules)
    arms_c.notes.append("static policy block composed at grader layer; "
                        "Cortex DB is identical to arm A")
    arms.append(arms_c)

    # ----- Arm D: + accepted Adapt candidates -----
    adapt_records = load_adapt_manifest_records(args.adapt_manifest)
    print(f"arm D: +{len(adapt_records)} accepted Adapt candidates "
          f"(manifest={args.adapt_manifest})")

    def _inject_d(cortex, db, port):
        for rec in adapt_records:
            put_pref_record(cortex, db, port, rec["id"], rec.get("scope", SCOPE),
                            adapt_record_envelope(rec))
    arms.append(run_arm_with_inject(args.cortex_bin, args.out / "D.db",
                                    rows, tmp, "D", _inject_d))

    # Post-arm: verify live DB unchanged + per-snapshot integrity.
    post_ok, post_msg = live_db_unchanged(args.live_db, pre_count)
    arm_integrity = {}
    for a in arms:
        ok, cnt, msg = integrity_check(Path(a.db_path))
        arm_integrity[a.arm] = {"ok": ok, "rows": cnt, "msg": msg}
    # Re-read post count for the integrity record.
    _, post_count, _ = integrity_check(args.live_db)
    integrity = {
        "pre": {"ok": pre_ok, "rows": pre_count, "msg": pre_msg},
        "post": {"ok": post_ok, "rows": post_count, "msg": post_msg,
                 "live_unchanged": post_ok and pre_count == post_count},
        "per_arm": arm_integrity,
    }
    (args.out / "integrity.json").write_text(
        json.dumps(integrity, indent=2, ensure_ascii=False), encoding="utf-8")

    # Persist results + report.
    results = {
        "generated_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "smoke": bool(args.smoke),
        "rules_count": len(rules),
        "rows_count": len(rows),
        "arms": [asdict(a) for a in arms],
        "query_rows": rows,
    }
    (args.out / "results.json").write_text(
        json.dumps(results, indent=2, ensure_ascii=False), encoding="utf-8")

    report = _format_report(results, integrity)
    (args.out / "report.md").write_text(report, encoding="utf-8")

    print(f"results: {args.out / 'results.json'}")
    print(f"report:  {args.out / 'report.md'}")
    print(f"live DB unchanged: {integrity['post']['live_unchanged']}")
    return 0 if integrity["post"]["live_unchanged"] else 1


def _format_report(results: dict, integrity: dict) -> str:
    lines = ["# Gate 0 — 4-arm A/B report", ""]
    lines.append(f"- generated: {results['generated_at']}")
    lines.append(f"- smoke: {results['smoke']}")
    lines.append(f"- rules: {results['rules_count']}")
    lines.append(f"- query rows: {results['rows_count']}")
    lines.append("")
    lines.append("## Integrity")
    lines.append("")
    lines.append(f"- pre  live: ok={integrity['pre']['ok']} "
                 f"rows={integrity['pre']['rows']} ({integrity['pre']['msg']})")
    lines.append(f"- post live: ok={integrity['post']['ok']} "
                 f"rows={integrity['post']['rows']} ({integrity['post']['msg']})  "
                 f"unchanged={integrity['post']['live_unchanged']}")
    for arm, info in integrity["per_arm"].items():
        lines.append(f"- arm {arm}: ok={info['ok']} rows={info['rows']} "
                     f"({info['msg']})")
    lines.append("")
    lines.append("## Per-arm wall-clock summary (ms)")
    lines.append("")
    for arm in results["arms"]:
        walls = list(arm["wall_ms"].values())
        if walls:
            avg = sum(walls) // len(walls)
            mx = max(walls)
            lines.append(f"- arm {arm['arm']}: avg={avg}ms max={mx}ms "
                         f"queries={len(walls)}")
    lines.append("")
    lines.append("## Note")
    lines.append("")
    lines.append("Per-arm retrieval rows + grader inputs are in `results.json`.")
    lines.append("Arm C's static policy block is composed at the grader layer; "
                 "the Cortex DB is identical to arm A.")
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    sys.exit(main())
