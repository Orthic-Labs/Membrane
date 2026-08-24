from adapt import consolidate_manifest, manifest


def _record(record_id: str, rule: str, evidence: str) -> dict:
    record = {
        "id": record_id,
        "rule": rule,
        "category": "workflow",
        "scope": "workspace",
        "record_type": "standing_preference",
        "authority_effect": "neutral",
        "status": "accepted",
        "confidence": 0.8,
        "needs_review": False,
        "evidence_count": 1,
        "evidence_excerpt": evidence,
        "source_ids": [f"source-{record_id}"],
        "source_file_hashes": [{"session_id": f"source-{record_id}", "sha256": "a" * 64}],
        "evidence_ids": [{
            "evidence_id": f"evidence-{record_id}",
            "source_session_id": f"source-{record_id}",
            "excerpt": evidence,
        }],
        "retrieval_aliases": [],
        "evidenceContexts": [{"sourceEventId": f"event-{record_id}"}],
        "payload_sha256": "",
    }
    record["payload_sha256"] = manifest.payload_sha256(record)
    return record


def test_parse_partition_rejects_missing_or_duplicate_ids():
    ids = {"one", "two"}
    assert consolidate_manifest._parse_partition('{"groups":[["one","two"]]}', ids) == [["one", "two"]]
    try:
        consolidate_manifest._parse_partition('{"groups":[["one"],["one"]]}', ids)
    except consolidate_manifest.ConsolidationError:
        pass
    else:
        raise AssertionError("duplicate partition must fail")


def test_parse_verdict_requires_boolean():
    assert consolidate_manifest._parse_verdict('{"equivalent":false,"reason":"different threshold"}') == (
        False, "different threshold",
    )
    try:
        consolidate_manifest._parse_verdict('{"equivalent":"yes"}')
    except consolidate_manifest.ConsolidationError:
        pass
    else:
        raise AssertionError("non-boolean verdict must fail")


def test_material_extension_is_not_equivalence():
    assert consolidate_manifest._has_material_extension([
        {"rule": "Never over-engineer."},
        {"rule": "Never over-engineer; keep architecture and implementation bounded and strictly on-scope."},
    ])
    assert not consolidate_manifest._has_material_extension([
        {"rule": "Keep all replies brief."},
        {"rule": "Keep answers brief."},
    ])


def test_consolidate_merges_evidence_without_rewriting_rule():
    first = _record("first", "Keep replies brief.", "brief")
    second = _record("second", "Keep answers brief.", "short")
    raw = {"records": [first, second], "generator": "test"}
    key = consolidate_manifest._bucket_key(first)
    result = consolidate_manifest.consolidate(raw, {key: [["first", "second"]]})
    merged = result["records"][0]
    assert merged["rule"] in {first["rule"], second["rule"]}
    assert merged["evidence_count"] == 2
    assert len(merged["source_ids"]) == 2
    assert merged["payload_sha256"] == manifest.payload_sha256(merged)
