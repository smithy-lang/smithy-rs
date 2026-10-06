"""Campaign identity must not leak through environment or another case's config."""
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from types import SimpleNamespace

import abfuzz


class ProtocolIsolationTest(unittest.TestCase):
    def test_replay_overrides_inherited_protocol_for_both_targets(self):
        with tempfile.TemporaryDirectory() as root, patch.object(abfuzz, "FUZZ_ROOT", root):
            for protocol in ("aws-json-10", "aws-json-11", "rest-json1", "rest-xml", "rpcv2-cbor"):
                directory = Path(root) / "work-isolated-v1" / "suite" / protocol
                directory.mkdir(parents=True)
                (directory / "smithy-fuzz-config.json").write_text(json.dumps({
                    "protocol": protocol,
                    "targets": [{"source": f"/generated/{side}", "shared_library": f"/{side}.so"}
                                for side in ("single", "multi")],
                }))
                response = SimpleNamespace(stdout=json.dumps({"response": {
                    "status": 200, "headers": {}, "body": []}, "input": "same"}), stderr=b"")
                with patch.dict(os.environ, {"SMITHY_FUZZ_PROTOCOL": "foreign"}), \
                        patch.object(abfuzz.subprocess, "run", return_value=response) as run:
                    abfuzz.Case("suite", protocol).invoke_file("case.bin")
                    self.assertEqual(run.call_count, 2)
                    for call in run.call_args_list:
                        self.assertEqual(call.kwargs["env"]["SMITHY_FUZZ_PROTOCOL"], protocol)
                        self.assertEqual(call.kwargs["cwd"], str(directory))

    def test_mismatched_campaign_is_rejected_before_invocation(self):
        with tempfile.TemporaryDirectory() as root, patch.object(abfuzz, "FUZZ_ROOT", root):
            directory = Path(root) / "work-isolated-v1" / "suite" / "aws-json-10"
            directory.mkdir(parents=True)
            (directory / "smithy-fuzz-config.json").write_text(json.dumps({
                "protocol": "aws-json-11", "targets": []}))
            with self.assertRaisesRegex(ValueError, "mismatched"):
                abfuzz.Case("suite", "aws-json-10").invoke_file("case.bin")


if __name__ == "__main__":
    unittest.main()
