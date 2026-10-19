# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Checks that missing results and unusable oracles cannot produce a clean audit."""
import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest

import run


class EvidenceCompletenessTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name)
        (self.path / "cases").mkdir()
        (self.path / "cases/rest-json.json").write_text(json.dumps([{"id": "control", "expected_legacy_status": 200}]))
        (self.path / "provenance.json").write_text(json.dumps({"protocols": ["rest-json"]}))
        (self.path / "compile-probes.tsv").write_text("probe\tlegacy\t0\nprobe\tschema\t0\n")
        self.row = dict(id="control", status=200, input={}, body={}, headers={}, wire_hex="7b7d", logs="", panic=None)
        self.write_rows("legacy", [self.row])
        self.write_rows("schema", [self.row])

    def write_rows(self, mode, rows):
        (self.path / f"rest-json-{mode}.jsonl").write_text("".join(json.dumps(r) + "\n" for r in rows))

    def check(self):
        with contextlib.redirect_stdout(io.StringIO()):
            return run.compare(self.path)

    def test_complete_equal_results_pass(self):
        self.assertFalse(self.check())

    def test_missing_or_duplicate_case_is_an_incomplete_run(self):
        for rows in ([], [self.row, self.row]):
            self.write_rows("schema", rows)
            with self.assertRaises(RuntimeError):
                self.check()

    def test_equal_rejections_cannot_replace_a_success_control(self):
        for mode in ("legacy", "schema"):
            self.write_rows(mode, [dict(self.row, status=415)])
        with self.assertRaises(RuntimeError):
            self.check()

    def test_different_inputs_and_logs_fail(self):
        for field, value in (("input", {"value": "different"}), ("logs", "missing diagnostic")):
            self.write_rows("schema", [dict(self.row, **{field: value})])
            self.assertTrue(self.check())

    def test_shared_panic_is_not_compatibility_success(self):
        for mode in ("legacy", "schema"):
            self.write_rows(mode, [dict(self.row, panic="boom")])
        self.assertTrue(self.check())

    def test_shared_compile_failure_is_not_success(self):
        (self.path / "compile-probes.tsv").write_text("probe\tlegacy\t101\nprobe\tschema\t101\n")
        self.assertTrue(self.check())


if __name__ == "__main__":
    unittest.main()
