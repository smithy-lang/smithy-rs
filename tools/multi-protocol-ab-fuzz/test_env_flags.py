# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Python and shell flags must agree with the Rust driver's presence checks."""
import json
import os
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch

import abfuzz


FLAGS = ("SMITHY_FUZZ_SEMANTIC_COMPARE", "SMITHY_FUZZ_IGNORE_UNROUTED")
VALUES = ("", "0", "false", "no", "off", " FALSE ", " No ", " OFF ",
          " 0 ", " \t\n", "1", "true", "yes", " enabled ", "\n FaLsE \r\n", "\n true \n")


class EnvironmentFlagsTest(unittest.TestCase):
    def test_python_values_and_unset_defaults(self):
        for flag in FLAGS:
            for value in (None, *VALUES):
                with self.subTest(flag=flag, value=value), patch.dict(os.environ, clear=True):
                    if value is not None:
                        os.environ[flag] = value
                    expected = value is not None and value.strip().lower() not in ("", "0", "false", "no", "off")
                    self.assertEqual(abfuzz.env_flag(flag), expected)

    def test_shell_normalization_and_python_import(self):
        directory = Path(__file__).resolve().parent
        code = (
            "import json, os, abfuzz; "
            "print(json.dumps([abfuzz.SEMANTIC, "
            "os.environ.get('SMITHY_FUZZ_SEMANTIC_COMPARE'), "
            "os.environ.get('SMITHY_FUZZ_IGNORE_UNROUTED')]))"
        )
        for value in (None, *VALUES):
            with self.subTest(value=value):
                env = {k: v for k, v in os.environ.items() if k not in FLAGS}
                env.update(BASELINE_REPO=str(directory), CURRENT_REPO=str(directory.parent.parent))
                if value is not None:
                    env.update({flag: value for flag in FLAGS})
                enabled = value is not None and value.strip().lower() not in ("", "0", "false", "no", "off")
                expected = [True, "1", None] if value is None else [enabled, "1" if enabled else None, None]
                result = subprocess.run(
                    ["bash", "-c", 'source "$1"; "$2" -c "$3"', "flags-test",
                     str(directory / "env.sh"), sys.executable, code],
                    cwd=directory, env=env, capture_output=True, text=True, check=True,
                )
                self.assertEqual(json.loads(result.stdout), expected)

    def test_direct_python_unset_flags_are_disabled(self):
        env = {k: v for k, v in os.environ.items() if k not in FLAGS}
        result = subprocess.run(
            [sys.executable, "-c", "import abfuzz; print(abfuzz.SEMANTIC)"],
            cwd=Path(__file__).resolve().parent, env=env,
            capture_output=True, text=True, check=True,
        )
        self.assertEqual(result.stdout.strip(), "False")


if __name__ == "__main__":
    unittest.main()
