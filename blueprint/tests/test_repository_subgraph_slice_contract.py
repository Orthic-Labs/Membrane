"""Plan 3.1: RepositorySubgraphSliceV1 contract test (Python side).

Validates the same golden fixture as the JS test. One fixture, two validators.
"""
import json
from pathlib import Path
import unittest

FIXTURE_PATH = Path(__file__).resolve().parents[1] / "schemas" / "goldens" / "repository-subgraph-slice-v1.fixture.json"

NODE_KINDS = {"file", "symbol", "region", "edge"}
CONFIDENCE_TIERS = {"EXACT_RESOLUTION", "STRONG_HEURISTIC", "CROSS_FILE_HEURISTIC", "UNRESOLVED"}


class RepositorySubgraphSliceV1ContractTest(unittest.TestCase):
    def setUp(self):
        with open(FIXTURE_PATH, "r", encoding="utf-8") as f:
            self.fixture = json.load(f)

    def test_top_level_fields(self):
        assert self.fixture["schemaVersion"] == 1
        assert self.fixture["kind"] == "RepositorySubgraphSliceV1"
        assert self.fixture["repoId"] is None or isinstance(self.fixture["repoId"], str)
        assert self.fixture["generationId"] is None or isinstance(self.fixture["generationId"], str)
        assert self.fixture["receiptId"] is None or isinstance(self.fixture["receiptId"], str)

    def test_seeds_structure(self):
        seeds = self.fixture["seeds"]
        assert len(seeds) > 0
        for seed in seeds:
            assert isinstance(seed["path"], str), f"seed path must be str: {seed}"
            assert isinstance(seed["reason"], str), f"seed reason must be str: {seed}"

    def test_nodes_structure(self):
        nodes = self.fixture["nodes"]
        assert len(nodes) > 0
        for node in nodes:
            assert isinstance(node["id"], str), f"node id must be str: {node}"
            assert isinstance(node["path"], str), f"node path must be str: {node}"
            assert node["kind"] in NODE_KINDS, f"node kind {node['kind']!r} not in {NODE_KINDS}"

    def test_edges_structure(self):
        edges = self.fixture["edges"]
        assert len(edges) > 0
        for edge in edges:
            assert isinstance(edge["id"], str), f"edge id must be str: {edge}"
            assert isinstance(edge["kind"], str), f"edge kind must be str: {edge}"
            assert isinstance(edge["source"], str), f"edge source must be str: {edge}"
            assert edge["confidenceTier"] in CONFIDENCE_TIERS, \
                f"edge confidenceTier {edge['confidenceTier']!r} not in {CONFIDENCE_TIERS}"

    def test_bounds_structure(self):
        bounds = self.fixture["bounds"]
        assert isinstance(bounds["maxNodes"], int)
        assert isinstance(bounds["maxEdges"], int)
        assert isinstance(bounds["maxHops"], int)
        assert isinstance(bounds["budgetTokens"], int)

    def test_omissions_structure(self):
        omissions = self.fixture["omissions"]
        for om in omissions:
            assert isinstance(om["reason"], str), f"omission reason must be str: {om}"

    def test_claims_structure(self):
        claims = self.fixture.get("claims", [])
        assert isinstance(claims, list)
        for claim in claims:
            assert isinstance(claim["id"], str), f"claim id must be str: {claim}"
            assert isinstance(claim["text"], str), f"claim text must be str: {claim}"

    def test_continuation_cursor_structure(self):
        cursor = self.fixture.get("continuationCursor")
        assert cursor is None or isinstance(cursor, str), \
            f"continuationCursor must be null or str: {cursor}"


if __name__ == "__main__":
    unittest.main()
