"""Taste — durable user preferences → MemRight."""
from __future__ import annotations

import datetime as dt
import json
import os
import shutil
import subprocess
import sys
import tempfile
from collections import defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import adapt_llm  # noqa: E402
import adapt_sessions as ts  # noqa: E402
try:
    import admission  # noqa: E402
except ImportError:
    admission = None
import preference_record  # noqa: E402
import manifest  # noqa: E402
import authority  # noqa: E402
import cross_machine  # noqa: E402
import rollback  # noqa: E402
import rule_key  # noqa: E402
import workspace_runtime  # noqa: E402


def _host_module():
    """Prefer the CLI facade when tests monkeypatch symbols there."""
    if "adapt_cli" in sys.modules:
        return sys.modules["adapt_cli"]
    return sys.modules.get("adapt")


def _host_attr(name: str, default):
    host = _host_module()
    if host is not None and hasattr(host, name):
        return getattr(host, name)
    return default

RULES_FILE = ts.STATE_DIR / "rules.json"
DIGEST_FILE = ts.STATE_DIR / "adapt-digest.md"
WORKSPACE_ROOT = workspace_runtime.workspace_root()
MEMRIGHT_MUTATION_TIMEOUT_SECONDS = 150


def _installation_file() -> Path:
    override = os.environ.get("ADAPT_INSTALLATION_FILE", "").strip()
    if override:
        return Path(override)
    root = _host_attr("WORKSPACE_ROOT", WORKSPACE_ROOT)
    return root / "tools/.cache/memory/installation.json"


def _multiwriter_context(
    *, manifest_body: dict | None = None, required: bool = False
) -> tuple[str, dict] | None:
    """Load the local UUID and canonical Adapt pool, or stay legacy before setup."""
    identity_path = _installation_file()
    if not identity_path.is_file():
        if required:
            raise cross_machine.CrossMachineAdaptError(
                "multiwriter manifest requires a local schema-v2 installation identity"
            )
        return None
    installation_id = cross_machine.load_installation_id(identity_path)
    db_path = rollback._discover_db_path(manifest_body or {})
    if db_path is None:
        raise cross_machine.CrossMachineAdaptError(
            "canonical MemRight DB is unavailable"
        )
    rules = cross_machine.load_canonical_rules(db_path)
    return installation_id, rules


def _qualified_session_sources(
    session_refs: list[dict], installation_id: str
) -> list[str]:
    return [
        cross_machine.qualify_source_session(
            installation_id,
            ref["tool"],
            ref.get("source_key") or ref["session_id"],
        )
        for ref in session_refs
    ]


def _rules_path() -> Path:
    """Late-bound rules.json path; monkeypatching ts.STATE_DIR works in tests."""
    return ts.STATE_DIR / "rules.json"


def _digest_path() -> Path:
    """Late-bound digest path; monkeypatching ts.STATE_DIR works in tests."""
    return ts.STATE_DIR / "adapt-digest.md"


def _audit_file() -> Path:
    """Compute audit target lazily, so tests can route writes via env override.

    Tests that exercise `_audit` should `os.environ["ADAPT_AUDIT_FILE_OVERRIDE"]`
    to a tmp_path before importing `adapt`. Operational runs have no override
    and write to the canonical `~/.claude/adapt/audit.jsonl`.
    """
    override = os.environ.get("ADAPT_AUDIT_FILE_OVERRIDE")
    if override:
        return Path(override)
    return ts.STATE_DIR / "audit.jsonl"


def _run_memright(args: list[str]) -> bool:
    """Invoke the installed memright shim (never touches tokens). Fail closed with the error."""
    bin_path = shutil.which("memright")
    if not bin_path:
        print("error: memright shim not on PATH; install via setup-workspace.py first",
              file=sys.stderr)
        return False
    try:
        command = list(args)
        if command and command[0] == "put" and "--artifact-family" not in command:
            command.extend([
                "--artifact-family", "adapt",
                "--producer", "adapt",
                "--record-type", "preference",
            ])
        res = subprocess.run(
            [bin_path, *command], capture_output=True, text=True,
            timeout=MEMRIGHT_MUTATION_TIMEOUT_SECONDS,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        print(f"error: memright shim failed: {exc}", file=sys.stderr)
        return False
    if res.returncode != 0:
        print(f"error: memright {args[0]} rc={res.returncode}: {res.stderr.strip()}",
              file=sys.stderr)
        return False
    return True


def preflight_apply(lane: str, allow_external: bool) -> bool:
    if not ts.scanner_available():
        print("error: adapt apply requires detect-secrets or gitleaks before transcript text can leave the machine",
              file=sys.stderr)
        return False
    if lane != "local" and not allow_external:
        print("error: external LLM lanes require --allow-external-lane", file=sys.stderr)
        return False
    if not adapt_llm.lane_available(lane):
        print(f"error: LLM lane unavailable: {lane}", file=sys.stderr)
        return False
    if not _host_attr("_run_memright", _run_memright)(["--help"]):
        print("error: memright shim is unavailable; run setup-workspace.py first", file=sys.stderr)
        return False
    return True


def initialized(state: dict) -> bool:
    return bool(state.get("initialized_at"))


def rule_body(rule: dict, evidence: str, tool: str) -> str:
    today = dt.date.today().isoformat()
    aliases = preference_record.normalize_retrieval_aliases(
        rule.get("retrieval_aliases", ()), rule=rule.get("rule", "")
    )
    alias_line = f"**Trigger phrases:** {' | '.join(aliases)}\n" if aliases else ""
    machine = (rule.get("machine") or "").strip()
    machine_line = (
        f"**Machine:** {machine}{' (machine-only)' if rule.get('machine_only') else ''}\n"
        if machine else ""
    )
    return (
        f"**[adapt/{rule['category']}]** — {rule['rule']} "
        f"Confidence: {rule['confidence']:.2f} "
        f"(observations: {rule['observations']}, needs_review: {str(rule.get('needs_review', False)).lower()}, "
        f"updated {today})\n"
        f"**Why:** mined from {tool or 'session'} prompts; e.g. \"{evidence}\"\n"
        f"**Record:** type={rule.get('record_type', 'unclassified')}, "
        f"authority_effect={rule.get('authority_effect', 'neutral')}\n"
        f"{alias_line}"
        f"{machine_line}"
        f"**How to apply:** {preference_record.application_guidance(rule.get('record_type', 'unclassified'))}\n"
    )


def _safe_retrieval_aliases(rule: str, observations: list[dict]) -> tuple[str, ...]:
    """Return bounded evidence cues only after the full alias payload scans clean."""
    aliases = preference_record.normalize_retrieval_aliases(
        [item.get("evidence", "") for item in observations], rule=rule
    )
    if not aliases:
        return ()
    payload = json.dumps(list(aliases), ensure_ascii=False)
    if not ts.scan_batch_for_secrets_str(payload):
        _audit({"event": "retrieval_aliases_scanner_blocked", "count": len(aliases)})
        return ()
    return aliases


def load_rules() -> dict:
    try:
        return json.loads(_rules_path().read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {}


def save_rules(rules: dict) -> None:
    ts.STATE_DIR.mkdir(parents=True, exist_ok=True)
    _rules_path().write_text(json.dumps(rules, indent=2), encoding="utf-8")


def write_digest(rules: dict, path: Path) -> None:
    by_cat: dict[str, list[dict]] = defaultdict(list)
    for r in rules.values():
        by_cat[r["category"]].append(r)
    lines = ["# Adapt digest (generated by adapt; MemRight rows are the source of truth)", ""]
    for cat in sorted(by_cat):
        lines.append(f"# {cat}")
        for r in sorted(by_cat[cat], key=lambda x: -x["confidence"]):
            lines.append(f"- {r['rule']} Confidence: {r['confidence']:.2f}")
        lines.append("")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines), encoding="utf-8")


def write_metrics(metrics: dict, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(metrics, indent=2), encoding="utf-8")


def _audit(entry: dict) -> None:
    ts.STATE_DIR.mkdir(parents=True, exist_ok=True)
    p = _audit_file()
    p.parent.mkdir(parents=True, exist_ok=True)
    with open(p, "a", encoding="utf-8") as fh:
        fh.write(json.dumps({"ts": dt.datetime.now(dt.timezone.utc).isoformat(), **entry}) + "\n")


def _scope_for(obs_list: list[dict]) -> str:
    scopes = {o.get("scope") for o in obs_list if o.get("scope")}
    # Never a hardcoded peer literal: writing another machine's scope is what
    # minted the duplicate rows cleaned up on 2026-07-26.
    return scopes.pop() if len(scopes) == 1 else ts.local_workspace_scope()


def _dimensions_for(obs_list: list[dict]) -> dict[str, str]:
    """AD1: structured facets for a synthesized rule.

    Only emitted when EVERY contributing observation agrees. A rule mined from
    two different repos is genuinely repo-agnostic, so narrowing it to one of
    them would silently stop it firing in the other — the exact failure mode
    scope_dimensions exists to prevent. Disagreement therefore yields {},
    meaning unqualified, meaning matches everything.
    """
    seen: set[tuple[tuple[str, str], ...]] = set()
    for obs in obs_list:
        obs_scope = obs.get("scope") or ""
        dims = ts.dimensions_for_scope(obs_scope) if obs_scope else {}
        seen.add(tuple(sorted(dims.items())))
    if len(seen) != 1:
        return {}
    return dict(next(iter(seen)))


def apply_actions(actions: list[dict], obs_by_cat: dict, rules: dict,
                  out_dir: Path, dry_run: bool,
                  manifest_records: list | None = None,
                  source_session_ids: list[str] | None = None,
                  source_file_hashes: dict[str, str] | None = None,
                  authority_manifest: dict | None = None,
                  authority_root: Path | None = None) -> tuple[int, bool]:
    """Apply actions.

    If `manifest_records` is provided, every action is appended as a
    schema-shaped candidate (Gate 1a manifest contract) keyed on a stable
    ``PreferenceRecord`` id with a ``payload_sha256`` over the immutable
    fields (Gate 1b hardening: source_file_hashes + evidence_ids are
    included). Automated adjudication resolves status; apply refuses content edits.

    ``source_session_ids`` (defaults to []) populates each candidate's
    ``source_ids`` so manifest apply can verify the rule's provenance.
    ``source_file_hashes`` (session_id → sha256) populates the
    ``source_file_hashes[]`` block so audits can pinpoint the transcript
    the rule was mined from.
    """
    changed = 0
    ok_all = True
    rule_index = rule_key.RuleIndex.from_mapping(rules)
    src_ids = list(source_session_ids or [])
    src_hashes = dict(source_file_hashes or {})
    current_machine = preference_record.default_machine_id()
    for act in actions:
        name, kind = act["name"], act["action"]
        if kind == "keep":
            continue
        category_observations = obs_by_cat.get(act["category"], [])
        requested_ids = act.get("observation_ids") or []
        obs_list = (
            [obs for obs in category_observations
             if obs.get("observation_id") in requested_ids]
            if requested_ids else category_observations
        )
        evidence = obs_list[0]["evidence"] if obs_list else ""
        tool = obs_list[0].get("tool", "") if obs_list else ""
        _resolved_key, existing_row = rule_index.resolve(
            name, scope=rules.get(name, {}).get("scope")
        )
        scope = (existing_row or {}).get("scope") or rules.get(name, {}).get("scope") or _scope_for(obs_list)
        scope_dimensions = _dimensions_for(obs_list)
        retrieval_aliases = _safe_retrieval_aliases(act["rule"], obs_list)
        linked_source_ids = list(dict.fromkeys(
            obs.get("session_id") for obs in obs_list if obs.get("session_id")
        ))
        candidate_source_ids = linked_source_ids or src_ids
        # Admission policy: refuse anything outside the controlled taxonomy,
        # empty/short/dup rules, or that doesn't pass minimum-shape checks.
        try:
            if admission is None:
                raise RuntimeError("admission module unavailable")
            admitted, why = admission.admit(
                kind,
                {**act, "scope": scope, "evidence_text": evidence},
                canonical_rules=rule_index,
                authority_manifest=authority_manifest,
                authority_root=authority_root,
                stored_rules=list(rules.values()),
            )
        except Exception as exc:
            admitted, why = False, f"admission-error:{type(exc).__name__}"
        existing_row = existing_row or rules.get(name) or {}
        # Attribution, not partitioning: preserve the recording machine
        # across updates (like created_at); a brand-new rule gets the
        # machine applying this run. machine_only never auto-flips True —
        # narrowing is an explicit operator decision (see add_rule).
        record = {"name": name, "category": act["category"], "rule": act["rule"],
                  "confidence": act["confidence"], "observations": act.get("observations", 1),
                  "scope": scope, "needs_review": act.get("needs_review", False),
                   "record_type": "standing_preference",
                   "retrieval_aliases": list(retrieval_aliases),
                  "authority_effect": authority.evaluate_rule(
                      act["rule"], scope=scope,
                      declared_effect=act.get("authority_effect")
                  ).authority_effect,
                  "machine": existing_row.get("machine") or current_machine,
                  "machine_only": bool(existing_row.get("machine_only", False))}
        # Manifest capture: schema-shaped candidate via PreferenceRecord.
        if manifest_records is not None:
            existing = existing_row or rules.get(name)
            pr = preference_record.PreferenceRecord.from_synthesis(
                {**act, "record_type": "standing_preference",
                 "retrieval_aliases": retrieval_aliases},
                scope=scope, source_ids=tuple(candidate_source_ids),
                existing=existing,
                machine=(existing or {}).get("machine") or current_machine,
                scope_dimensions=scope_dimensions,
            )
            cand = preference_record.to_manifest_candidate(
                pr, evidence_excerpt=evidence,
                status="pending",
                operation=kind,
            )
            # Gate 1b: per-session transcript hashes + stable evidence IDs.
            cand["source_file_hashes"] = [
                {"session_id": sid, "sha256": src_hashes[sid]}
                for sid in candidate_source_ids if sid in src_hashes
            ]
            cand["evidence_ids"] = [
                {
                    "evidence_id": manifest.derive_evidence_id(
                        scope, obs.get("evidence", "")
                    ),
                    "source_session_id": obs.get("session_id", ""),
                    "excerpt": obs.get("evidence", ""),
                }
                for obs in obs_list if obs.get("evidence")
            ]
            if authority_manifest:
                cand["authority_manifest_sha256"] = authority_manifest[
                    "manifest_sha256"
                ]
            # Compute and stamp payload_sha256 AFTER the Gate 1b additions so
            # the loader's candidate_payload() computes the same hash.
            cand["payload_sha256"] = manifest.payload_sha256(cand)
            cand["_admission_status"] = "pending" if admitted else "rejected-by-admission"
            cand["_rejection_reason"] = None if admitted else why
            cand["_action"] = kind
            manifest_records.append(cand)
        if not admitted:
            print(f"  reject  {name}  cat={act.get('category','?')}  reason={why}")
            _audit({"event": "admission_rejected", "name": name, "why": why,
                    "category": act.get("category", "")})
            continue
        print(f"  {kind:6s} {name}  conf={act['confidence']:.2f}  [{scope}]")
        if dry_run:
            continue
        body = rule_body(record, evidence, tool)
        with tempfile.NamedTemporaryFile("w", suffix=".md", delete=False,
                                         encoding="utf-8", dir=str(out_dir)) as tmp:
            tmp.write(body)
            tmp_path = tmp.name
        ok = _host_attr("_run_memright", _run_memright)(["put", name, "--scope", scope, "--file", tmp_path])
        Path(tmp_path).unlink(missing_ok=True)
        if ok:
            rules[name] = record
            rule_index = rule_key.RuleIndex.from_mapping(rules)
            _audit({"action": kind, **record})
            changed += 1
        else:
            ok_all = False
            _audit({"event": "memright_write_failed", "action": kind, "name": name, "scope": scope})
    return changed, ok_all

def add_rule(rule_text: str, category: str, *, record_type: str = "operational_playbook",
             scope: str | None = None, dry_run: bool = False,
             machine_only: bool = False) -> int:
    """Operator-authored single-rule add — the lightweight path that skips mining.

    Adapt's discover->extract(LLM)->synthesize(LLM) pipeline exists to pull rules
    out of MESSY transcripts. When the operator already has one clean rule, there is
    nothing to mine — so this goes straight to the SAME admission gate (category,
    non-empty, dedup, min-length, authority/permission-expansion quarantine) and, if
    admitted, writes the MemRight row + local rules state + digest.

    Default record_type is `operational_playbook`: a domain gotcha surfaces via
    query-relevant recall (cos>=0.40) but stays OUT of the always-on compiled core.
    Pass record_type='standing_preference' ONLY for a truly universal rule that must
    apply to every prompt (then run --compile-core to fold it into the core).

    `machine_only=True` narrows the rule to the recording machine alone; the
    default (False) is today's unqualified, workspace-wide behavior. The
    recording machine itself is always attributed via
    `preference_record.default_machine_id()` regardless of this flag.
    """
    rule_text = (rule_text or "").strip()
    # Resolve at call time, not in the signature: a module-level default would
    # bake one machine's slug in at import. Never a hardcoded peer literal.
    scope = scope or ts.local_workspace_scope()
    cat = admission.normalize_category(category)
    rtype = authority.normalize_record_type(record_type)
    rid = preference_record.derive_id(scope, cat, rule_text)

    existing = load_rules()
    rule_index = rule_key.RuleIndex.from_mapping(existing)

    candidate = {"name": rid, "rule": rule_text, "category": cat, "scope": scope,
                 "authority_effect": "neutral"}
    admitted, why = admission.admit("add", candidate, canonical_rules=rule_index)
    if not admitted:
        print(f"rejected: {why}  (category={cat}, id={rid})", file=sys.stderr)
        return 1

    now = dt.datetime.now(dt.timezone.utc).isoformat()
    record = {
        "schema_version": preference_record.SCHEMA_VERSION,
        "id": rid, "kind": "add", "rule": rule_text, "category": cat, "scope": scope,
        "confidence": 0.9, "needs_review": False, "observations": 1, "evidence_count": 1,
        "source_ids": [rid], "created_at": now, "updated_at": now,
        "record_type": rtype, "authority_effect": "neutral", "status": "accepted",
        "retrieval_aliases": [],
        "machine": preference_record.default_machine_id(),
        "machine_only": bool(machine_only),
        # A user-authored rule enters the store already active — it was stated
        # explicitly, not inferred, so it does not sit in `candidate`. Stamping
        # verification at creation is what makes `never_verified()` meaningful:
        # without it every direct rule looks unverified forever and the freshness
        # surface is pure noise.
        "lifecycle_state": preference_record.normalize_lifecycle_state("active"),
        "last_verified_at": now,
        "verification_count": 1,
    }

    core_note = ("  -> in the always-on core after --compile-core" if rtype == "standing_preference"
                 else "  -> recall-gated (surfaces on a query match); NOT always-on")
    if dry_run:
        print(f"[dry-run] would add {rid}  type={rtype}{core_note}")
        return 0

    body = rule_body(record, evidence="operator-added", tool="operator")
    with tempfile.NamedTemporaryFile("w", suffix=".md", delete=False, encoding="utf-8") as tmp:
        tmp.write(body)
        tmp_path = tmp.name
    ok = _host_attr("_run_memright", _run_memright)(["put", rid, "--scope", scope, "--file", tmp_path])
    Path(tmp_path).unlink(missing_ok=True)
    if not ok:
        return 1

    existing[rid] = record
    save_rules(existing)
    _audit({"action": "operator_add", **record})
    write_digest(existing, _digest_path())
    print(f"added {rid}  type={rtype}{core_note}")
    return 0
