import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parents[2] / "packages/python/src"))
from membrane_client import MembraneClient, ProtocolError, analyze_packet, analyze_receipt


def envelope(operation, result):
    return {"schemaVersion": 1, "operation": operation, "errorVersion": 1, "result": result}


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
