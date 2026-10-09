"""Replay should retain launch errors without reusing another target's result."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import abfuzz


class InvocationErrorsTest(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        root = Path(temp.name)
        directory = root / "work-isolated-v1" / "suite" / "rest-json1"
        directory.mkdir(parents=True)
        (directory / "smithy-fuzz-config.json").write_text(json.dumps({
            "protocol": "rest-json1",
            "targets": [
                {"source": "/generated/single", "shared_library": "/single.so"},
                {"source": "/generated/multi", "shared_library": "/multi.so"},
            ],
        }))
        with patch.object(abfuzz, "FUZZ_ROOT", str(root)):
            self.case = abfuzz.Case("suite", "rest-json1")

    def response(self, stderr=b""):
        return subprocess.CompletedProcess(
            args=["aws-smithy-fuzz"], returncode=0,
            stdout=json.dumps({
                "response": {"status": 200, "headers": {"x-test": ["ok"]}, "body": [65]},
                "input": "example",
            }).encode(), stderr=stderr,
        )

    def test_first_launch_failure_preserves_error_and_runs_other_target(self):
        with patch.object(abfuzz.subprocess, "run", side_effect=[
            FileNotFoundError("aws-smithy-fuzz unavailable"), self.response(),
        ]) as run:
            single, multi = self.case.invoke_file("case.bin")
        self.assertEqual(single, {"failure": "invocation failed: aws-smithy-fuzz unavailable"})
        self.assertEqual(multi["status"], 200)
        self.assertEqual(run.call_count, 2)

    def test_second_launch_failure_does_not_reuse_first_stderr(self):
        with patch.object(abfuzz.subprocess, "run", side_effect=[
            self.response(stderr=b"prior target stderr"),
            PermissionError("second target launch denied"),
        ]):
            single, multi = self.case.invoke_file("case.bin")
        self.assertEqual(single["status"], 200)
        self.assertEqual(multi, {"failure": "invocation failed: second target launch denied"})

    def test_timeout_keeps_hang_classification(self):
        with patch.object(abfuzz.subprocess, "run", side_effect=[
            subprocess.TimeoutExpired("aws-smithy-fuzz", 30), self.response(),
        ]):
            single, multi = self.case.invoke_file("case.bin")
        self.assertEqual(single, {"failure": "hang (>30s)"})
        self.assertEqual(multi["status"], 200)

    def test_malformed_output_retains_panic_diagnostic(self):
        failure = subprocess.CompletedProcess(
            args=["aws-smithy-fuzz"], returncode=1, stdout=b"not JSON",
            stderr=b"context\nthread 'main' panicked at example\nmore context",
        )
        with patch.object(abfuzz.subprocess, "run", side_effect=[failure, self.response()]):
            single, multi = self.case.invoke_file("case.bin")
        self.assertEqual(single, {"failure": "panic: thread 'main' panicked at example"})
        self.assertEqual(multi["status"], 200)

    def test_malformed_output_without_panic_retains_stderr(self):
        failure = subprocess.CompletedProcess(
            args=["aws-smithy-fuzz"], returncode=1, stdout=b"not JSON", stderr=b"driver error",
        )
        with patch.object(abfuzz.subprocess, "run", side_effect=[failure, self.response()]):
            single, _ = self.case.invoke_file("case.bin")
        self.assertEqual(single, {"failure": "panic: driver error"})

    def test_successful_responses_are_preserved(self):
        with patch.object(abfuzz.subprocess, "run", return_value=self.response()):
            single, multi = self.case.invoke_file("case.bin")
        expected = {
            "status": 200, "headers": {"x-test": ["ok"]}, "body": b"A", "input": "example",
        }
        self.assertEqual(single, expected)
        self.assertEqual(multi, expected)


if __name__ == "__main__":
    unittest.main()
