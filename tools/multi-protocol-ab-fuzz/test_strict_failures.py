# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Regressions and failed background jobs must make validation fail."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import abfuzz


class StrictFailureTests(unittest.TestCase):
    def test_routing_suppression_environment_cannot_hide_a_difference(self):
        baseline = {"input": None, "status": 404, "headers": {}, "body": b""}
        candidate = {"input": None, "status": 400, "headers": {}, "body": b"{}"}
        with patch.dict(os.environ, {"SMITHY_FUZZ_IGNORE_UNROUTED": "1"}), \
                patch.object(abfuzz, "SEMANTIC", True):
            self.assertIsNotNone(abfuzz.describe(baseline, candidate))

    def test_comparison_cli_commands_exit_nonzero_for_differences(self):
        commands = {
            "corpus-diff": ("cmd_corpus_diff", ["suite", "case"]),
            "compatibility-diff": ("cmd_compatibility_diff", ["suite", "case"]),
            "replay": ("cmd_replay", ["suite", "case"]),
            "cluster": ("cmd_cluster", ["suite", "case"]),
            "probe": ("cmd_probe", ["suite", "case", "fixture.py"]),
        }
        for command, (function, arguments) in commands.items():
            for differing in (0, 1):
                with self.subTest(command=command, differing=differing), \
                        patch.object(abfuzz, function, return_value=differing), \
                        self.assertRaises(SystemExit) as raised:
                    abfuzz.main(["abfuzz.py", command, *arguments])
                self.assertEqual(raised.exception.code, differing)

    def test_background_script_failures_are_not_lost(self):
        directory = Path(__file__).resolve().parent
        for script in ("init.sh", "fuzz.sh", "replay.sh"):
            with self.subTest(script=script), tempfile.TemporaryDirectory() as temporary:
                env = dict(os.environ)
                env.update(
                    BASELINE_REPO=str(directory),
                    CURRENT_REPO=str(directory.parent.parent),
                    FUZZ_ROOT=temporary,
                    SUITES="missing-suite",
                    CASES="missing-first missing-second",
                    JOBS="1",
                )
                result = subprocess.run(
                    ["bash", str(directory / script), "1", "1"],
                    env=env, capture_output=True, text=True,
                )
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                if script != "replay.sh":
                    self.assertIn("missing-first", result.stdout)
                    self.assertIn("missing-second", result.stdout)


if __name__ == "__main__":
    unittest.main()
