import json
import sys
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).parents[2]
sys.path.insert(0, str(REPO_ROOT / "dist/packages/python/src"))
from membrane_client import MembraneClient, ProtocolError, analyze_packet, analyze_receipt


def envelope(operation, result):
    return {"schemaVersion": 1, "operation": operation, "errorVersion": 1, "result": result}


def golden(name):
    """Load the exact same canonical v1 fixture the TypeScript
    (tests/sdk/cross-language-compat.test.mjs) and Rust
    (engine/crates/membrane-client/tests/compat.rs) compatibility suites
    read, so all three SDKs are proven against identical fixture bytes
    rather than independently authored equivalents."""
    with open(REPO_ROOT / "operations/operations" / name, encoding="utf-8") as handle:
        return json.load(handle)


class PythonClientGoldenFixtureTest(unittest.TestCase):
    """MBR-308: same shared fixtures as the TypeScript and Rust suites."""

    def test_context_golden_success_matches_the_shared_fixture(self):
        response = golden("membrane-context.v1.golden.json")
        client = MembraneClient(lambda *_: response)
        self.assertEqual(client.context({}), response["result"]["data"])

    def test_context_golden_error_is_typed_and_retryable(self):
        response = golden("membrane-context.v1.error.golden.json")
        client = MembraneClient(lambda *_: response)
        with self.assertRaises(ProtocolError) as error:
            client.context({})
        self.assertEqual(error.exception.code, "context_deadline_exceeded")
        self.assertTrue(error.exception.retryable)

    def test_source_read_golden_success_matches_the_shared_fixture(self):
        response = golden("membrane-source-read.v1.golden.json")
        client = MembraneClient(lambda *_: response)
        self.assertEqual(client.source_read({}), response["result"]["data"])

    def test_source_read_golden_error_is_typed_and_not_retryable(self):
        response = golden("membrane-source-read.v1.error.golden.json")
        client = MembraneClient(lambda *_: response)
        with self.assertRaises(ProtocolError) as error:
            client.source_read({})
        self.assertEqual(error.exception.code, "source_read_hash_mismatch")
        self.assertFalse(error.exception.retryable)

    def test_every_indexed_error_code_round_trips_for_each_operation(self):
        index = golden("operations-index.v1.golden.json")
        by_name = {entry["name"]: entry["errorCodes"] for entry in index["operations"]}
        for operation in ("membrane_context", "membrane_source_read"):
            for code in by_name[operation]:
                response = envelope(operation, {"kind": "error", "code": code, "message": "typed", "retryable": False})
                client = MembraneClient(lambda *_ , response=response: response)
                with self.assertRaises(ProtocolError) as error:
                    client.call(operation, {})
                self.assertEqual(error.exception.code, code)


class PythonClientTest(unittest.TestCase):
    def test_success_uses_only_injected_transport(self):
        calls = []
        client = MembraneClient(lambda op, body: calls.append((op, body)) or envelope(op, {"kind": "success", "data": {"id": "p"}}))
        self.assertEqual(client.context({"task": "x"}), {"id": "p"})
        self.assertEqual(calls, [("membrane_context", {"task": "x"})])

    def test_known_remote_error_is_typed(self):
        client = MembraneClient(lambda op, _: envelope(op, {"kind": "error", "code": "context_scope_denied", "message": "denied", "retryable": False}))
        with self.assertRaisesRegex(ProtocolError, "denied") as error:
            client.context({})
        self.assertEqual(error.exception.code, "context_scope_denied")

    def test_rejects_version_operation_and_unknown_error(self):
        for response in (
            {"schemaVersion": 2, "operation": "membrane_context", "errorVersion": 1, "result": {"kind": "success", "data": {}}},
            envelope("membrane_source_read", {"kind": "success", "data": {}}),
            envelope("membrane_context", {"kind": "error", "code": "new_error", "message": "x", "retryable": False}),
        ):
            with self.assertRaises(ProtocolError):
                MembraneClient(lambda *_: response).context({})

    def test_receipt_supports_current_and_prior_versions_only(self):
        base = {"admittedChars": 12, "decision": "admitted", "providerStatus": "fresh", "fallbackMode": "none", "degradationReason": "none"}
        self.assertFalse(analyze_receipt({"schemaVersion": 1, **base}).degraded)
        self.assertTrue(analyze_receipt({"schemaVersion": 2, **base, "providerStatus": "stale"}).degraded)
        with self.assertRaises(ProtocolError):
            analyze_receipt({"schemaVersion": 3, **base})

    def test_packet_analysis_and_closed_envelopes(self):
        self.assertEqual(analyze_packet({"schemaVersion": 1, "blocks": [1], "omissions": []}).included, 1)
        response = {**envelope("membrane_context", {"kind": "success", "data": {}}), "raw": "leak"}
        with self.assertRaises(ProtocolError):
            MembraneClient(lambda *_: response).context({})


if __name__ == "__main__":
    unittest.main()
