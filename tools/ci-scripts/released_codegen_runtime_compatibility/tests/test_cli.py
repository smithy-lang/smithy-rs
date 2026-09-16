# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from released_codegen_runtime_compatibility.models import PublishedCodegen
from released_codegen_runtime_compatibility import verify_compatibility


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
    def test_arguments_default_to_latest_codegen(self) -> None:
        """Default to latest codegen while accepting an exact published version."""
        args = entrypoint.parse_args([])
        self.assertIsNone(args.codegen_version)

        historical = entrypoint.parse_args(["--codegen-version", "0.1.20"])
        self.assertEqual("0.1.20", historical.codegen_version)

    def test_main_directly_calls_compatibility_verifier(self) -> None:
        """Make the entrypoint's relationship to its verification workflow explicit."""
        with tempfile.TemporaryDirectory() as repository, mock.patch.object(
            entrypoint, "verify_released_codegen_runtime_compatibility"
        ) as verifier:
            entrypoint.main(
                [
                    "--repository-root",
                    repository,
                    "--codegen-version",
                    "0.1.20",
                ]
            )

        verifier.assert_called_once_with(
            repository_root=Path(repository),
            runtime_root=None,
            codegen_version="0.1.20",
        )


class VerificationTest(unittest.TestCase):
    def test_temporary_prefix_includes_codegen_version(self) -> None:
        """Include the exact published codegen version in generation work directories."""
        self.assertEqual(
            "smithy-rs-codegen-compat-0.1.24-",
            verify_compatibility.temporary_prefix("0.1.24"),
        )

    def test_verification_runs_each_compatibility_step(self) -> None:
        """Resolve, generate, patch, and compile through explicit domain operations."""
        with tempfile.TemporaryDirectory() as repository:
            repository_root = Path(repository)
            runtime_root = repository_root / "custom-runtime"
            published = PublishedCodegen("0.1.24", "1.73.0")
            generated = object()
            patched = object()
            with mock.patch.object(
                verify_compatibility,
                "resolve_published_codegen",
                return_value=published,
            ) as resolve, mock.patch.object(
                verify_compatibility,
                "generate_protocol_sdks",
                return_value=generated,
            ) as generate, mock.patch.object(
                verify_compatibility,
                "patch_generated_sdks_with_current_runtimes",
                return_value=patched,
            ) as patch, mock.patch.object(
                verify_compatibility, "compile_generated_sdks"
            ) as compile_sdks:
                verify_compatibility.verify_released_codegen_runtime_compatibility(
                    repository_root=repository_root,
                    runtime_root=runtime_root,
                )

        resolve.assert_called_once_with(None)
        generate.assert_called_once_with(
            published_codegen=published,
            protocols=verify_compatibility.COMPATIBILITY_PROTOCOLS,
            repository_root=repository_root.resolve(),
            destination=mock.ANY,
        )
        patch.assert_called_once_with(
            generated_sdks=generated,
            runtime_root=runtime_root.resolve(),
            destination_root=mock.ANY,
        )
        compile_sdks.assert_called_once_with(patched)

        generation_destination = generate.call_args.kwargs["destination"]
        verification_destination = patch.call_args.kwargs["destination_root"]
        self.assertEqual("generation", generation_destination.name)
        self.assertEqual("verification", verification_destination.name)
        self.assertEqual(generation_destination.parent, verification_destination.parent)


if __name__ == "__main__":
    unittest.main()
