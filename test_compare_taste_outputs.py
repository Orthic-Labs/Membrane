import importlib.util
import json
from pathlib import Path

import numpy as np


MODULE_PATH = Path(__file__).parent / "eval" / "compare_taste_outputs.py"


def _module():
    spec = importlib.util.spec_from_file_location("compare_taste_outputs", MODULE_PATH)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_loaders_filter_morph_and_parse_taste(tmp_path):
    module = _module()
    results = tmp_path / "results.json"
    results.write_text(json.dumps({"rules": [
        {"rule": "Keep me.", "status": "candidate", "record_type": "standing_preference"},
        {"rule": "Quarantine me.", "status": "quarantined", "record_type": "locked_decision"},
    ]}), encoding="utf-8")
    taste = tmp_path / "taste.md"
    taste.write_text("# workflow\n- Keep me. Confidence: 0.85\n", encoding="utf-8")

    assert module.load_morph_rules(results, status="candidate") == ["Keep me."]
    assert module.parse_taste_rules(taste) == ["Keep me."]


def test_compare_vectors_uses_optimal_one_to_one_assignment():
    module = _module()

    class Embedder:
        def embed_documents(self, texts):
            vectors = {"a": [1.0, 0.0], "b": [0.0, 1.0],
                       "x": [0.9, 0.1], "y": [0.1, 0.9]}
            result = np.array([vectors[text] for text in texts], dtype=np.float32)
            return result / np.linalg.norm(result, axis=1, keepdims=True)

    result = module.compare_vectors(["a", "b"], ["x", "y"], Embedder())

    assert result["matched"] == 2
    assert result["pairs"][0]["similarity"] > 0.99
    assert {pair["a_rule"] for pair in result["pairs"]} == {"a", "b"}


def test_without_exact_removes_only_baseline_strings():
    module = _module()

    assert module.without_exact(["same", "changed", "new"], ["same", "old"]) == [
        "changed", "new"
    ]


def test_internal_pairs_reports_only_unique_pairs_above_threshold():
    module = _module()

    class Embedder:
        def embed_documents(self, texts):
            vectors = {"a": [1.0, 0.0], "near": [0.99, 0.1], "far": [0.0, 1.0]}
            result = np.array([vectors[text] for text in texts], dtype=np.float32)
            return result / np.linalg.norm(result, axis=1, keepdims=True)

    pairs = module.internal_pairs(["a", "near", "far"], Embedder(), threshold=0.8)

    assert len(pairs) == 1
    assert {pairs[0]["left_rule"], pairs[0]["right_rule"]} == {"a", "near"}
