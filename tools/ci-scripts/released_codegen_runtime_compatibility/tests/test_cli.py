# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from released_codegen_runtime_compatibility.models import PublishedCodegen


ENTRYPOINT_PATH = (
    Path(__file__).resolve().parents[2] / "released-codegen-runtime-compatibility.py"
)
ENTRYPOINT_SPEC = importlib.util.spec_from_file_location(
    "released_codegen_runtime_compatibility_entrypoint", ENTRYPOINT_PATH
)
if ENTRYPOINT_SPEC is None or ENTRYPOINT_SPEC.loader is None:
    raise RuntimeError("failed to load {}".format(ENTRYPOINT_PATH))
entrypoint = importlib.util.module_from_spec(ENTRYPOINT_SPEC)
ENTRYPOINT_SPEC.loader.exec_module(entrypoint)


class CliTest(unittest.TestCase):
    def test_verify_semver_command(self) -> None:
        """Default to latest codegen while accepting an exact published version."""
        args = entrypoint.parse_args(["verify-semver"])
        self.assertIs(entrypoint.verify_semver, args.handler)
        self.assertIsNone(args.codegen_version)

        historical = entrypoint.parse_args(
            ["verify-semver", "--codegen-version", "0.1.20"]
        )
        self.assertEqual("0.1.20", historical.codegen_version)

    def test_temporary_prefix_includes_codegen_version(self) -> None:
        """Include the exact published codegen version in generation work directories."""
        self.assertEqual(
            "smithy-rs-codegen-compat-0.1.24-",
            entrypoint.temporary_prefix("0.1.24"),
        )

    def test_verify_semver_always_generates_temporary_sdks(self) -> None:
        """Generate SDKs from published JARs and pass them directly to Cargo verification."""
        with tempfile.TemporaryDirectory() as repository:
            args = entrypoint.parse_args(
                ["--repository-root", repository, "verify-semver"]
            )
            published = PublishedCodegen("0.1.24", "1.73.0")
            generated = object()
            with mock.patch.object(
                entrypoint, "ProtocolSdkGenerator"
            ) as generator, mock.patch.object(
                entrypoint, "CargoVerifier"
            ) as verifier, mock.patch.object(
                entrypoint, "MavenCodegenResolver"
            ) as resolver:
                resolver.return_value.resolve.return_value = published
                generator.return_value.generate.return_value = generated

                entrypoint.verify_semver(args)

                resolver.return_value.resolve.assert_called_once_with(None)
                generation_args = generator.return_value.generate.call_args[0]
                self.assertEqual(published, generation_args[0])
                verifier.return_value.verify.assert_called_once_with(
                    generated, mock.ANY
                )


if __name__ == "__main__":
    unittest.main()
