import importlib.util
import json
from pathlib import Path

import numpy as np


MODULE_PATH = Path(__file__).parent / "eval" / "build_conflict_review_packet.py"


def _module():
    spec = importlib.util.spec_from_file_location("build_conflict_review_packet", MODULE_PATH)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class FakeEmbedder:
    def embed_documents(self, texts):
        vectors = []
        for text in texts:
            lower = text.casefold()
            vectors.append([
                float("jsonl" in lower), float("blocker" in lower), 0.1,
            ])
        result = np.array(vectors, dtype=np.float32)
        return result / np.linalg.norm(result, axis=1, keepdims=True)


def test_packets_are_candidate_only_retrieved_and_coverage_checkable(tmp_path):
    module = _module()
    results = tmp_path / "results.json"
    results.write_text(json.dumps({"rules": [
        {
            "name": "taste-workflow-1", "record_type": "standing_preference",
            "category": "workflow", "scope": "D--Claude", "status": "candidate",
            "rule": "Keep JSONL logs.", "support": [],
        },
        {
            "name": "taste-safety-2", "record_type": "locked_decision",
            "category": "safety", "scope": "D--Claude", "status": "candidate",
            "rule": "Respect hard blockers.", "support": [],
        },
        {
            "name": "quarantined", "status": "quarantined", "rule": "Do not include me."
        },
    ]}), encoding="utf-8")
    authority = tmp_path / "authority"
    authority.mkdir()
    (authority / "AGENTS.md").write_text(
        "Use JSONL for logs.\n\nStop only for hard blockers.", encoding="utf-8"
    )

    packets = module.build_packets(
        results, authority, FakeEmbedder(), top_k=1, batch_size=1
    )

    assert len(packets) == 2
    assert packets[0]["expected_names"] == ["taste-workflow-1"]
    assert "Use JSONL for logs." in packets[0]["text"]
    assert "quarantined" not in packets[0]["text"]
    assert '"reviewed_rule_count":1' in packets[0]["text"]


def test_near_rule_pairs_are_attached_across_batches():
    module = _module()
    rules = [
        {"name": "a", "rule": "Keep JSONL logs."},
        {"name": "b", "rule": "Use JSONL logging."},
        {"name": "c", "rule": "Respect blockers."},
    ]

    pairs = module.near_rule_pairs(rules, FakeEmbedder(), threshold=0.8)

    assert pairs["a"][0]["rule_name"] == "b"
    assert pairs["b"][0]["rule_name"] == "a"
    assert pairs["c"] == []


def test_packets_can_select_an_exact_candidate_subset(tmp_path):
    module = _module()
    results = tmp_path / "results.json"
    results.write_text(json.dumps({"rules": [
        {"name": "a", "status": "candidate", "rule": "Keep JSONL logs."},
        {"name": "b", "status": "candidate", "rule": "Respect blockers."},
    ]}), encoding="utf-8")
    authority = tmp_path / "authority"
    authority.mkdir()
    (authority / "AGENTS.md").write_text("Respect blockers.", encoding="utf-8")

    packets = module.build_packets(
        results, authority, FakeEmbedder(), top_k=1, batch_size=5, names={"b"}
    )

    assert packets[0]["expected_names"] == ["b"]
    assert '"name": "a"' not in packets[0]["text"]
