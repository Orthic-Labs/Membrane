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
    def test_simd_runner_smoke_emits_full_parity_checked_matrix(self):
        runner = HARNESS_PATH.parent / "simd" / "Cargo.toml"
        config = HARNESS_PATH.parent / "config" / "round1-v1.json"
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "simd.json"
            completed = subprocess.run(
                [
                    "cargo",
                    "run",
                    "--manifest-path",
                    str(runner),
                    "--locked",
                    "--release",
                    "--",
                    "--config",
                    str(config),
                    "--cell",
                    "smoke",
                    "--output",
                    str(output),
                ],
                capture_output=True,
                text=True,
                timeout=300,
            )
            self.assertEqual(completed.returncode, 0, completed.stderr)
            bundle = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(
                [arm["arm"] for arm in bundle["arms"]],
                ["A", "B2", "B3", "parallel-B3", "parallel-B2", "B3-SIMD", "B2-gather"],
            )
            for arm in bundle["arms"]:
                self.assertTrue(arm["exact"])
            if sys.platform == "win32":
                self.assertEqual(
                    bundle["arms"][1]["config"]["kernel"],
                    "rust-avx2-fma-intrinsics",
                )
                self.assertEqual(
                    bundle["arms"][1]["backend"],
                    "rust-avx2-fma-full-scores-bounded-topn",
                )
            elif sys.platform == "darwin":
                arms = {arm["arm"]: arm for arm in bundle["arms"]}
                self.assertEqual(
                    arms["parallel-B2"]["backend"],
                    "rayon-accelerate-sgemv-full-scores-bounded-topn",
                )
                self.assertEqual(
                    arms["B2-gather"]["backend"],
                    "eligible-row-gather-accelerate-sgemv-bounded-topn",
                )

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


if __name__ == "__main__":
    unittest.main()
