from __future__ import annotations

import hashlib
from pathlib import Path

from federation.providers import rules


def _write_agents_md(tmp_path: Path, content: str) -> Path:
    p = tmp_path / "AGENTS.md"
    p.write_text(content, encoding="utf-8")
    return p


def test_self_loading_client_gets_reference_only_candidates(tmp_path: Path):
    _write_agents_md(tmp_path, "some workspace rules content\n" * 5)

    candidates = rules.produce(tmp_path, "test", client="claude")

    assert len(candidates) == 1
    candidate = candidates[0]
    assert candidate["text"] == ""
    assert candidate["recoverable"] is True
    assert candidate["resolver"] == "read AGENTS.md"


def test_non_self_loading_client_gets_content_inline(tmp_path: Path):
    content = "some workspace rules content\n" * 5
    _write_agents_md(tmp_path, content)

    candidates = rules.produce(tmp_path, "test", client="some-api-worker")

    assert len(candidates) == 1
    assert candidates[0]["text"] == content


def test_empty_client_defaults_to_content_mode(tmp_path: Path):
    content = "some workspace rules content\n"
    _write_agents_md(tmp_path, content)

    candidates_no_client = rules.produce(tmp_path, "test")
    candidates_empty_client = rules.produce(tmp_path, "test", client="")

    assert candidates_no_client[0]["text"] == content
    assert candidates_empty_client[0]["text"] == content


def test_unknown_client_defaults_to_content_mode(tmp_path: Path):
    content = "some workspace rules content\n"
    _write_agents_md(tmp_path, content)

    candidates = rules.produce(tmp_path, "test", client="totally-unknown-host")

    assert candidates[0]["text"] == content


def test_reference_mode_has_fewer_estimated_tokens_than_content_mode(tmp_path: Path):
    _write_agents_md(tmp_path, "some workspace rules content\n" * 50)

    content_candidates = rules.produce(tmp_path, "test", client="worker")
    reference_candidates = rules.produce(tmp_path, "test", client="claude")

    assert (
        reference_candidates[0]["estimatedTokens"]
        < content_candidates[0]["estimatedTokens"]
    )


def test_source_hash_is_real_sha256_and_changes_with_content(tmp_path: Path):
    content_a = "version a\n"
    path = _write_agents_md(tmp_path, content_a)

    candidates_a = rules.produce(tmp_path, "test", client="worker")
    expected_hash_a = hashlib.sha256(content_a.encode("utf-8")).hexdigest()
    assert candidates_a[0]["sourceHash"] == expected_hash_a
    assert candidates_a[0]["sourceHash"] != "0" * 64

    content_b = "version b, materially different\n"
    path.write_text(content_b, encoding="utf-8")
    candidates_b = rules.produce(tmp_path, "test", client="worker")
    expected_hash_b = hashlib.sha256(content_b.encode("utf-8")).hexdigest()
    assert candidates_b[0]["sourceHash"] == expected_hash_b
    assert candidates_b[0]["sourceHash"] != candidates_a[0]["sourceHash"]


def test_content_mode_truncates_files_larger_than_cap(tmp_path: Path):
    big_content = "x" * (rules.CAP_BYTES + 500)
    _write_agents_md(tmp_path, big_content)

    candidates = rules.produce(tmp_path, "test", client="worker")

    assert candidates[0]["truncated"] is True
    assert candidates[0]["text"].endswith("[TRUNCATED]")
    assert len(candidates[0]["text"]) <= rules.CAP_BYTES + len("\n[TRUNCATED]")


def test_content_mode_small_file_not_truncated(tmp_path: Path):
    _write_agents_md(tmp_path, "small content\n")

    candidates = rules.produce(tmp_path, "test", client="worker")

    assert candidates[0]["truncated"] is False
    assert "[TRUNCATED]" not in candidates[0]["text"]


def test_candidate_shape_preserves_all_expected_keys(tmp_path: Path):
    _write_agents_md(tmp_path, "content\n")

    for client in ("claude", "worker", ""):
        candidates = rules.produce(tmp_path, "test", client=client)
        candidate = candidates[0]
        expected_keys = {
            "id",
            "layer",
            "sourceKind",
            "sourceRef",
            "sourceHash",
            "trustClass",
            "instructionPolicy",
            "providerScore",
            "scoreComponents",
            "estimatedTokens",
            "protected",
            "exact",
            "recoverable",
            "resolver",
            "truncated",
            "text",
        }
        assert expected_keys.issubset(candidate.keys())
        assert candidate["id"] == "rules:AGENTS.md"
        assert candidate["layer"] == 2
        assert candidate["sourceKind"] == "doc"
        assert candidate["sourceRef"] == "AGENTS.md"
        assert candidate["trustClass"] == "workspace_tracked"
        assert candidate["instructionPolicy"] == "data_only"
        assert candidate["providerScore"] == 0.3
        assert candidate["scoreComponents"] == {"rule_relevance": 0.3}
        assert candidate["protected"] is False
        assert candidate["exact"] is False
