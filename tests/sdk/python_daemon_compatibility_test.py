"""MBR-909 acceptance: "Python SDK passes compatibility tests against current
and previous supported daemon."

The only envelope in the shared protocol that currently has more than one
live schema version is the per-candidate receipt: `ContextReceiptV1.schemaVersion`
is documented and typed as `1 or 2` in
schemas/context-receipt.v1.schema.json ("the planner always populates the v2
observability fields" — i.e. 2 is what a current daemon emits, 1 is what a
previous supported daemon emitted and the client must still accept). This
test reads that enum directly from the schema at test time, rather than
hard-coding [1, 2] a second time, so a future daemon version bump (the enum
growing to include 3, or dropping 1 once it is no longer supported) changes
this test's coverage automatically instead of silently going stale — the
same "checked against the shared schema, not review" property MBR-308 (see
python_client_test.py's PythonClientGoldenFixtureTest) applied to the
operation-envelope fixtures.

`operations/operations/*.golden.json` (schemaVersion/errorVersion) is
intentionally out of scope here: every current operation envelope declares
exactly one schema version (1), so there is no second, older supported
version of that contract for a "current vs previous daemon" test to exercise
yet. When operations-index.v1.golden.json starts listing more than one
schemaVersion for an operation, that becomes the same kind of enum-driven
test this file already applies to the receipt.
"""

import json
import sys
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).parents[2]
sys.path.insert(0, str(REPO_ROOT / "dist/packages/python/src"))
from membrane_client import ProtocolError, analyze_receipt  # noqa: E402


def _load_json(path):
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def _base_receipt(schema_version):
    return {
        "schemaVersion": schema_version,
        "admittedChars": 12,
        "decision": "admitted",
        "providerStatus": "fresh",
        "fallbackMode": "none",
        "degradationReason": "none",
    }


class PythonClientSupportsCurrentAndPreviousDaemonTest(unittest.TestCase):
    def setUp(self):
        schema = _load_json(REPO_ROOT / "schemas" / "context-receipt.v1.schema.json")
        self.supported_versions = schema["properties"]["schemaVersion"]["enum"]

    def test_schema_still_advertises_a_current_and_a_previous_version(self):
        """Guards the premise of this whole test file: if the schema ever
        drops back to a single supported version, that is a real product
        change this suite should surface loudly rather than silently pass
        a single-element loop."""
        self.assertGreaterEqual(len(self.supported_versions), 2)

    def test_client_accepts_every_daemon_version_the_schema_lists_as_supported(self):
        for version in self.supported_versions:
            with self.subTest(schemaVersion=version):
                result = analyze_receipt(_base_receipt(version))
                self.assertEqual(result.schema_version, version)
                self.assertFalse(result.degraded)

    def test_client_fails_closed_against_a_daemon_newer_than_any_supported_version(self):
        unsupported = max(self.supported_versions) + 1
        with self.assertRaises(ProtocolError) as error:
            analyze_receipt(_base_receipt(unsupported))
        self.assertEqual(error.exception.code, "protocol_version_unsupported")

    def test_previous_supported_daemon_degradation_semantics_still_hold(self):
        """The previous supported daemon (schemaVersion == min(supported))
        is still a fully valid, non-degraded receipt on its own; degradation
        is signalled by providerStatus/fallbackMode/degradationReason, not
        merely by being the older of the two supported versions."""
        oldest = min(self.supported_versions)
        self.assertFalse(analyze_receipt(_base_receipt(oldest)).degraded)


if __name__ == "__main__":
    unittest.main()
