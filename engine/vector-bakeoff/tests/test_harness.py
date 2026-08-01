import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


HARNESS_PATH = Path(__file__).resolve().parents[1] / "harness.py"
SPEC = importlib.util.spec_from_file_location("vector_bakeoff_harness", HARNESS_PATH)
assert SPEC and SPEC.loader
harness = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(harness)


def bundle(runner="control", cell="smoke", fixture="f" * 64):
    return {
        "schemaVersion": 1,
        "generatorId": "memright-vector-fixture-v1",
        "runner": runner,
        "runnerVersion": "0.1.0",
        "cellId": cell,
        "fixtureSha256": fixture,
        "rows": 2,
        "queries": 1,
        "dimension": 2,
        "arms": [
            {
                "arm": "A",
                "exact": True,
                "researchOnly": False,
                "measurements": [{"queryId": 0, "candidateIds": [1]}],
                "config": {},
            }
        ],
    }


class HarnessTests(unittest.TestCase):
    def test_input_digest_detects_byte_drift(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            path = root / "input.json"
            path.write_text("one", encoding="utf-8")
            first = harness.input_digest([path], root)
            path.write_text("two", encoding="utf-8")
            self.assertNotEqual(first, harness.input_digest([path], root))

    def test_atomic_json_leaves_complete_document(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "receipt.json"
            harness.atomic_json(path, {"status": "complete"})
            self.assertEqual(json.loads(path.read_text()), {"status": "complete"})
            self.assertFalse(path.with_suffix(".json.tmp").exists())

    def test_bundle_rejects_fixture_drift(self):
        with self.assertRaisesRegex(harness.HarnessError, "fixture drift"):
            harness.validate_bundle(
                bundle(), runner="control", cell="smoke", expected_fixture="0" * 64
            )

    def test_resume_requires_matching_input_and_result_hashes(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "result.json"
            output.write_text("{}", encoding="utf-8")
            entry = {
                "inputDigest": "digest",
                "resultSha256": harness.sha256_file(output),
            }
            self.assertTrue(harness.resume_valid(entry, output, "digest"))
            output.write_text("changed", encoding="utf-8")
            self.assertFalse(harness.resume_valid(entry, output, "digest"))

    def test_runner_timeout_is_classified(self):
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(harness.HarnessError, "runner timeout"):
                harness.run_command(
                    [sys.executable, "-c", "import time; time.sleep(1)"],
                    env=dict(os.environ),
                    timeout_seconds=0.01,
                    log_path=Path(temporary) / "timeout.log",
                )

    def test_compare_requires_same_inputs_and_exact_candidates(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            receipts = []
            for host in ("mac", "windows"):
                directory = root / host
                result = directory / "smoke" / "control.json"
                harness.atomic_json(result, bundle())
                receipt = {
                    "schemaVersion": 1,
                    "mode": "smoke",
                    "status": "complete",
                    "inputDigest": "sealed",
                    "configSha256": "config",
                    "manifestSha256": "manifest",
                    "host": {"system": host},
                    "results": [
                        {
                            "cell": "smoke",
                            "runner": "control",
                            "path": "smoke/control.json",
                            "fixtureSha256": "f" * 64,
                        }
                    ],
                }
                receipt_path = directory / "receipt.json"
                harness.atomic_json(receipt_path, receipt)
                receipts.append(receipt_path)
            report = harness.compare_receipts(*receipts)
            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["exactArmChecks"], 1)

    def test_simd_release_smoke_enforces_mac_parity_contracts(self):
        if sys.platform != "darwin":
            self.skipTest("macOS Accelerate contract")
        root = HARNESS_PATH.parent
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "simd.json"
            subprocess.run(
                [
                    "cargo",
                    "run",
                    "--manifest-path",
                    str(root / "simd" / "Cargo.toml"),
                    "--locked",
                    "--release",
                    "--",
                    "--config",
                    str(root / "config" / "round1-v1.json"),
                    "--cell",
                    "smoke",
                    "--output",
                    str(output),
                ],
                check=True,
                cwd=root,
                capture_output=True,
                text=True,
            )
            result = json.loads(output.read_text(encoding="utf-8"))
            arms = {arm["arm"]: arm for arm in result["arms"]}
            self.assertEqual(
                set(arms),
                {
                    "A",
                    "B2",
                    "B2-gather",
                    "parallel-B2",
                    "B3",
                    "B3-SIMD",
                    "parallel-B3",
                    "B4-f16",
                },
            )
            self.assertEqual(arms["B2"]["config"]["kernel"], "Accelerate cblas_sgemv")
            self.assertIn("neon-sdot", arms["B3-SIMD"]["config"]["kernel"])
            for control, b2, gather, parallel_b2, b3, simd, parallel_b3, f16 in zip(
                arms["A"]["measurements"],
                arms["B2"]["measurements"],
                arms["B2-gather"]["measurements"],
                arms["parallel-B2"]["measurements"],
                arms["B3"]["measurements"],
                arms["B3-SIMD"]["measurements"],
                arms["parallel-B3"]["measurements"],
                arms["B4-f16"]["measurements"],
            ):
                self.assertEqual(control["candidateIds"], b3["candidateIds"])
                self.assertEqual(control["candidateIds"], simd["candidateIds"])
                self.assertEqual(control["candidateIds"], parallel_b3["candidateIds"])
                self.assertTrue(b2["targetPresent"])
                self.assertTrue(gather["targetPresent"])
                self.assertTrue(parallel_b2["targetPresent"])
                self.assertTrue(f16["targetPresent"])
                self.assertGreaterEqual(
                    len(set(control["candidateIds"]) & set(f16["candidateIds"])),
                    len(control["candidateIds"]) - 1,
                )


if __name__ == "__main__":
    unittest.main()
