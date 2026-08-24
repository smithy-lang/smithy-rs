# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from released_codegen_runtime_compatibility.cargo import (
    _append_runtime_patches,
    _discover_runtime_crates,
)
from released_codegen_runtime_compatibility.models import RuntimeCrate


def runtime_path(*parts: str) -> Path:
    """Build one absolute runtime crate path for whichever platform runs the tests.
    Resolve it the same way discovery does so comparisons hold on Windows too.
    """
    return Path("/repo/rust-runtime").joinpath(*parts).resolve()


def toml_basic_string(value: str) -> str:
    """Escape one value the way a TOML basic string requires.
    Re-derive the expected rendering rather than reusing the code under test.
    """
    return '"{}"'.format(value.replace("\\", "\\\\").replace('"', '\\"'))


class CargoPatchingTest(unittest.TestCase):
    def test_runtime_discovery_uses_cargo_metadata(self) -> None:
        """Verify Cargo metadata drives current runtime patch discovery.
        Ensure unpublished and non-AWS workspace packages are excluded.
        """
        runtime_root = Path("/repo/rust-runtime")
        metadata = {
            "packages": [
                {
                    "name": "aws-one",
                    "publish": None,
                    "manifest_path": "/repo/rust-runtime/aws-one/Cargo.toml",
                },
                {
                    "name": "aws-two",
                    "publish": [],
                    "manifest_path": "/repo/rust-runtime/aws-two/Cargo.toml",
                },
                {
                    "name": "not-aws",
                    "publish": None,
                    "manifest_path": "/repo/rust-runtime/not-aws/Cargo.toml",
                },
            ]
        }
        with mock.patch(
            "released_codegen_runtime_compatibility.cargo.output",
            return_value=(0, json.dumps(metadata), ""),
        ) as output_mock:
            crates = _discover_runtime_crates(runtime_root)

        self.assertEqual(
            [RuntimeCrate(name="aws-one", path=runtime_path("aws-one"))],
            crates,
        )
        output_mock.assert_called_once_with(
            [
                "cargo",
                "metadata",
                "--no-deps",
                "--format-version",
                "1",
                "--manifest-path",
                runtime_root / "Cargo.toml",
            ],
            runtime_root,
        )

    def test_append_runtime_patches(self) -> None:
        """Verify current runtime paths are rendered into a crates.io patch table.
        Ensure existing lockfiles are removed before compatibility resolution.
        """
        with tempfile.TemporaryDirectory() as temp:
            workspace = Path(temp)
            (workspace / "Cargo.toml").write_text("[workspace]\nmembers = []\n")
            (workspace / "Cargo.lock").write_text("old lock")
            crate_path = Path(temp) / 'path with "quotes"'
            _append_runtime_patches(
                workspace,
                [
                    RuntimeCrate(
                        name="aws-smithy-example",
                        path=crate_path,
                    )
                ],
            )
            contents = (workspace / "Cargo.toml").read_text()
            self.assertIn("[patch.crates-io]", contents)
            self.assertIn(
                "aws-smithy-example = {{ path = {} }}".format(
                    toml_basic_string(str(crate_path))
                ),
                contents,
            )
            self.assertFalse((workspace / "Cargo.lock").exists())


if __name__ == "__main__":
    unittest.main()
