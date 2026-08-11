"""Manifest loader + validator for adapt (Gate 1a contract).

The manifest is the *only* path from dry-run output to live apply. Gate 1a
requires that the loader refuses:

  - unknown schema versions,
  - records whose ``payload_sha256`` mismatches their actual content,
  - any record with ``status == "pending"`` (apply-time inputs must be explicit),
  - missing batch_id or empty source_session_ids.

  An empty ``records`` array is allowed and commits as a no-op batch.

The loader never makes LLM/provider calls. It is the gate that prevents
silently altered content from reaching Crypt.

Zero-record manifests are valid committed no-ops: a successfully mined batch
may bind source sessions without emitting durable candidates.

Helpers:
  - ``candidate_payload(record)`` — the immutable fields that ``payload_sha256``
    is computed over. If anything edits this set after emission, the hash
    no longer matches and apply refuses.
  - ``payload_sha256(record)`` — the canonical hash.
  - ``load_and_validate(path)`` — schema + invariant checks.
  - ``accepted_records(manifest)`` / ``rejected_records(manifest)``.
"""
from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

SCHEMA_PATH = Path(__file__).resolve().parent / "preference-manifest.schema.json"


# ----- Immutable candidate payload -----

# These fields are part of the contract's "immutable until regenerated" set.
# Adjudication can flip status and add an audit note, but cannot edit these.
IMMUTABLE_FIELDS = (
    "id",
    "rule",
    "category",
    "scope",
    "source_ids",
    "source_file_hashes",
    "evidence_ids",
    "evidence_count",
    "evidence_excerpt",
    "record_type",
    "authority_effect",
    "authority_manifest_sha256",
    "retrieval_aliases",
    "evidenceContexts",
)


def derive_evidence_id(scope: str, excerpt: str) -> str:
    """Stable evidence ID: ``ev-{sha256(scope + NUL + excerpt)[:8]}``.

    NUL separator prevents identical-looking excerpts from colliding when
    their scopes differ.
    """
    import hashlib as _h
    h = _h.sha256()
    h.update((scope or "").encode("utf-8"))
    h.update(b"\x00")
    h.update((excerpt or "").encode("utf-8"))
    return f"ev-{h.hexdigest()[:8]}"


def _normalize_source_file_hashes(rec: dict) -> list[dict]:
    items = rec.get("source_file_hashes") or []
    norm = [{"session_id": str(it.get("session_id", "")),
             "sha256": str(it.get("sha256", "")).lower()}
            for it in items]
    norm.sort(key=lambda x: x["session_id"])
    return norm


def _normalize_evidence_ids(rec: dict) -> list[dict]:
    items = rec.get("evidence_ids") or []
    norm = [{"evidence_id": str(it.get("evidence_id", "")),
             "source_session_id": str(it.get("source_session_id", "")),
             "excerpt": str(it.get("excerpt", ""))}
            for it in items]
    norm.sort(key=lambda x: x["evidence_id"])
    return norm


def candidate_payload(record: dict) -> dict:
    """Stable projection used to compute payload_sha256.

    Lists are sorted so list-order edits don't change the hash.
    """
    payload = {
        "id": record["id"],
        "rule": record["rule"],
        "category": record["category"],
        "scope": record["scope"],
        "source_ids": sorted(record.get("source_ids") or []),
        "source_file_hashes": _normalize_source_file_hashes(record),
        "evidence_ids": _normalize_evidence_ids(record),
        "evidence_count": int(record.get("evidence_count", 0)),
        "evidence_excerpt": record.get("evidence_excerpt", "") or "",
    }
    # v1.0 records omitted these fields. Preserve their historical hashes;
    # v1.1 records include both fields in the immutable signed payload.
    if "record_type" in record:
        payload["record_type"] = record["record_type"]
    if "authority_effect" in record:
        payload["authority_effect"] = record["authority_effect"]
    if "authority_manifest_sha256" in record:
        payload["authority_manifest_sha256"] = record["authority_manifest_sha256"]
    if "retrieval_aliases" in record:
        payload["retrieval_aliases"] = sorted({
            " ".join(str(value).split())
            for value in (record.get("retrieval_aliases") or [])
            if str(value).strip()
        })
    if "evidenceContexts" in record:
        payload["evidenceContexts"] = record["evidenceContexts"]
    return payload


def payload_sha256(record: dict) -> str:
    p = candidate_payload(record)
    return hashlib.sha256(
        json.dumps(p, sort_keys=True, ensure_ascii=False).encode("utf-8")
    ).hexdigest()


# ----- Validation -----

class ManifestError(ValueError):
    """Raised when a manifest fails schema or invariant checks."""


def _load_schema() -> dict:
    return json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))


def validate_schema(path: Path) -> dict:
    """Parse + schema-only validation. Lets ``pending`` status through.

    Use this for inspection of a freshly-emitted (still pending) manifest.
    For apply-time use ``apply_time_validate``, which additionally rejects
    pending and verifies every ``payload_sha256`` matches.
    """
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise ManifestError(f"manifest is not valid JSON: {exc}") from exc
    try:
        import jsonschema
        schema = _load_schema()
        jsonschema.validate(raw, schema)
    except ImportError:
        raise ManifestError(
            "jsonschema not installed; `pip install jsonschema` to validate manifests"
        )
    except jsonschema.ValidationError as exc:
        raise ManifestError(f"manifest schema check failed: {exc.message}") from exc
    if raw.get("schema_version") in {"1.1.0", "1.3.0"}:
        for rec in raw.get("records", []):
            missing = [
                field for field in ("record_type", "authority_effect")
                if field not in rec
            ]
            if missing:
                raise ManifestError(
                    "manifest schema check failed: v1.1.0 record "
                    f"{rec.get('id', '<unknown>')} missing {', '.join(missing)}"
                )
    if raw.get("schema_version") == "1.3.0":
        missing = [rec.get("id", "<unknown>") for rec in raw.get("records", []) if not rec.get("evidenceContexts")]
        if missing:
            raise ManifestError(f"manifest schema check failed: v1.3.0 records missing evidenceContexts: {missing}")
        refs = raw.get("source_refs") or []
        ids = [ref["source_id"] for ref in refs]
        if len(ids) != len(set(ids)):
            raise ManifestError("manifest source_refs contains duplicate source_id")
        if raw.get("source_session_ids") != ids:
            raise ManifestError("manifest source_session_ids must exactly match source_refs order")
        hashes = {ref["source_id"]: ref["source_sha256"].removeprefix("sha256:") for ref in refs}
        for rec in raw.get("records", []):
            rec_ids = rec.get("source_ids") or []
            if len(rec_ids) != len(set(rec_ids)) or any(value not in hashes for value in rec_ids):
                raise ManifestError(f"manifest record {rec.get('id', '<unknown>')} has unknown or duplicate source_ids")
            file_hashes = rec.get("source_file_hashes") or []
            if len({item.get("session_id") for item in file_hashes}) != len(file_hashes):
                raise ManifestError(f"manifest record {rec.get('id', '<unknown>')} has duplicate source_file_hashes")
            for item in file_hashes:
                sid, digest = item.get("session_id"), str(item.get("sha256") or "").lower()
                if sid not in hashes or hashes[sid] != digest:
                    raise ManifestError(f"manifest record {rec.get('id', '<unknown>')} source hash does not match source_refs")
            for context in rec.get("evidenceContexts") or []:
                source_events = [event for event in context["contextEvents"] if event["isSource"]]
                if len(source_events) != 1:
                    raise ManifestError(f"manifest record {rec.get('id', '<unknown>')} evidence context must contain exactly one source event")
                source = source_events[0]
                expected = {
                    "eventId": context["sourceEventId"], "kind": context["sourceKind"],
                    "role": context["sourceRole"], "classification": context["sourceClassification"],
                    "flags": context["sourceFlags"], "byteStart": context["sourceByteStart"],
                    "byteEnd": context["sourceByteEnd"], "text": context["evidenceText"],
                }
                for field, value in expected.items():
                    if source[field] != value:
                        raise ManifestError(f"manifest record {rec.get('id', '<unknown>')} evidence context source mismatch: {field}")
    frozen_authority = raw.get("authority_manifest")
    if frozen_authority:
        expected = frozen_authority["manifest_sha256"]
        unbound = [
            rec.get("id", "<unknown>")
            for rec in raw.get("records", [])
            if rec.get("authority_manifest_sha256") != expected
        ]
        if unbound:
            raise ManifestError(
                "manifest schema check failed: records are not bound to the "
                f"embedded authority manifest: {unbound}"
            )
    return raw


def apply_time_validate(path: Path) -> dict:
    """Parse + schema-validate + invariant-check. Apply-time validator.

    Refuses any record with ``status='pending'``, refuses edited payloads
    (payload_sha256 mismatch), and refuses missing fields. A zero-record
    manifest is a valid committed no-op for a successfully mined batch.
    For inspection of a freshly-emitted manifest, use
    ``validate_schema`` instead.

    Returns the parsed manifest dict on success. Raises ``ManifestError``.
    """
    raw = validate_schema(path)

    # Invariant: no record's content can have been edited post-emission.
    bad = []
    for rec in raw["records"]:
        if rec["payload_sha256"] != payload_sha256(rec):
            bad.append(rec["id"])
    if bad:
        raise ManifestError(
            f"manifest payload_sha256 mismatch on records: {bad}; "
            f"regenerate the manifest instead of editing it"
        )

    # Invariant: apply-time manifest must contain ONLY explicit decisions.
    statuses = {r["status"] for r in raw["records"]}
    if "pending" in statuses:
        raise ManifestError(
            "manifest contains status='pending'; resolve every record "
            "to accepted/rejected before applying"
        )
    return raw


# Backwards-compatible alias. adapt.apply_from_manifest + tests use this name.
def load_and_validate(path: Path) -> dict:
    return apply_time_validate(path)


def accepted_records(manifest: dict) -> list[dict]:
    return [r for r in manifest["records"] if r["status"] == "accepted"]


def rejected_records(manifest: dict) -> list[dict]:
    return [r for r in manifest["records"] if r["status"] == "rejected"]


# ----- CLI for manual inspection -----

def main(argv: list[str] | None = None) -> int:
    import argparse
    ap = argparse.ArgumentParser(description="Validate an adapt preference manifest")
    ap.add_argument("manifest", type=Path)
    ap.add_argument("--summary", action="store_true",
                    help="print accepted/rejected counts instead of full listing")
    args = ap.parse_args(argv)

    try:
        # CLI inspection accepts pending — use schema-only validator.
        manifest = validate_schema(args.manifest)
    except ManifestError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2

    if args.summary:
        a = len(accepted_records(manifest))
        r = len(rejected_records(manifest))
        print(f"batch_id={manifest['batch_id']} "
              f"sessions={len(manifest['source_session_ids'])} "
              f"accepted={a} rejected={r}")
        return 0

    print(f"batch_id: {manifest['batch_id']}")
    print(f"created_at: {manifest.get('created_at', '')}")
    print(f"source_session_ids ({len(manifest['source_session_ids'])}):")
    for sid in manifest["source_session_ids"]:
        print(f"  - {sid}")
    print(f"records ({len(manifest['records'])}):")
    for rec in manifest["records"]:
        marker = "ACCEPT" if rec["status"] == "accepted" else "REJECT"
        print(f"  [{marker}] {rec['id']}")
        print(f"      rule: {rec['rule']}")
        print(f"      category: {rec['category']}  scope: {rec['scope']}")
        if rec.get("human_note"):
            print(f"      note: {rec['human_note']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
