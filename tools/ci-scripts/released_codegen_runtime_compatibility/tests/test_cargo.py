# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from released_codegen_runtime_compatibility.cargo import (
    _append_runtime_patches,
    _patch_selection_problems,
    _resolved_runtime_packages,
    _satisfies_requirement,
    _verify_runtime_selection,
    discover_runtime_crates,
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
                    "version": "0.60.1",
                    "publish": None,
                    "manifest_path": "/repo/rust-runtime/aws-one/Cargo.toml",
                },
                {
                    "name": "aws-two",
                    "version": "0.1.0",
                    "publish": [],
                    "manifest_path": "/repo/rust-runtime/aws-two/Cargo.toml",
                },
                {
                    "name": "not-aws",
                    "version": "0.1.0",
                    "publish": None,
                    "manifest_path": "/repo/rust-runtime/not-aws/Cargo.toml",
                },
            ]
        }
        with mock.patch(
            "released_codegen_runtime_compatibility.cargo.output",
            return_value=(0, json.dumps(metadata), ""),
        ) as output_mock:
            crates = discover_runtime_crates(runtime_root)

        self.assertEqual(
            [
                RuntimeCrate(
                    name="aws-one", path=runtime_path("aws-one"), version="0.60.1"
                )
            ],
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
                        version="0.60.0",
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


CRATES_IO = "registry+https://github.com/rust-lang/crates.io-index"


class RequirementSatisfactionTest(unittest.TestCase):
    def test_caret_requirements(self) -> None:
        """Apply Cargo's leftmost-non-zero caret rule to plain requirements."""
        self.assertTrue(_satisfies_requirement("0.60.3", "0.60.1"))
        self.assertTrue(_satisfies_requirement("0.60.3", "^0.60"))
        self.assertFalse(_satisfies_requirement("0.61.0", "0.60.1"))
        self.assertFalse(_satisfies_requirement("0.60.0", "0.60.1"))
        self.assertTrue(_satisfies_requirement("1.4.0", "1.2.3"))
        self.assertFalse(_satisfies_requirement("2.0.0", "1.2.3"))
        self.assertTrue(_satisfies_requirement("0.0.3", "0.0.3"))
        self.assertFalse(_satisfies_requirement("0.0.4", "0.0.3"))

    def test_exact_requirements(self) -> None:
        """Honor exact requirements independently of the caret rule."""
        self.assertTrue(_satisfies_requirement("1.2.3", "=1.2.3"))
        self.assertFalse(_satisfies_requirement("1.2.4", "=1.2.3"))

    def test_unsupported_operator_is_surfaced(self) -> None:
        """Fail loudly on requirement operators codegen never emits."""
        with self.assertRaises(RuntimeError):
            _satisfies_requirement("1.2.3", ">=1.0")


class RuntimeSelectionTest(unittest.TestCase):
    CANDIDATES = {
        "aws-smithy-json": RuntimeCrate(
            name="aws-smithy-json",
            path=Path("/repo/rust-runtime/aws-smithy-json"),
            version="0.63.2",
        )
    }

    def test_registry_selection_that_patch_should_have_won_fails(self) -> None:
        """Flag a crates.io selection the checkout candidate also satisfies."""
        problems = _patch_selection_problems(
            self.CANDIDATES,
            {"aws-smithy-json": {("0.63.0", CRATES_IO)}},
            {"aws-smithy-json": {("generated-client", "0.63.0")}},
        )
        self.assertEqual(1, len(problems))
        self.assertIn("aws-smithy-json 0.63.0 resolved from crates.io", problems[0])

    def test_incompatible_registry_selection_is_expected(self) -> None:
        """Accept crates.io selections the checkout candidate cannot satisfy.

        This happens legitimately when HEAD moved a runtime crate to a new
        incompatible version line: released generated code must keep using
        the released version.
        """
        candidates = {
            "aws-smithy-json": RuntimeCrate(
                name="aws-smithy-json",
                path=Path("/repo/rust-runtime/aws-smithy-json"),
                version="0.64.0",
            )
        }
        problems = _patch_selection_problems(
            candidates,
            {"aws-smithy-json": {("0.63.0", CRATES_IO)}},
            {"aws-smithy-json": {("generated-client", "0.63.0")}},
        )
        self.assertEqual([], problems)

    def test_missing_expected_runtime_crate_fails(self) -> None:
        """Fail when the model stops exercising an expected runtime crate."""
        metadata = {"packages": []}
        with mock.patch(
            "released_codegen_runtime_compatibility.cargo.output",
            return_value=(0, json.dumps(metadata), ""),
        ):
            with self.assertRaises(RuntimeError) as caught:
                _verify_runtime_selection(
                    "client", Path("/workspace"), list(self.CANDIDATES.values())
                )
        self.assertIn("no longer exercises expected runtime crates", str(caught.exception))
        self.assertIn("aws-smithy-eventstream", str(caught.exception))

    def test_patched_selection_passes_and_summarizes(self) -> None:
        """Pass when every expected crate resolves to the checkout candidates."""
        from released_codegen_runtime_compatibility.cargo import (
            EXPECTED_RUNTIME_CRATES,
        )

        candidates = [
            RuntimeCrate(
                name=name,
                path=Path("/repo/rust-runtime") / name,
                version="0.60.0",
            )
            for name in sorted(EXPECTED_RUNTIME_CRATES["client"])
        ]
        metadata = {
            "packages": [
                {
                    "name": crate.name,
                    "version": crate.version,
                    "source": None,
                    "dependencies": [],
                }
                for crate in candidates
            ]
        }
        with mock.patch(
            "released_codegen_runtime_compatibility.cargo.output",
            return_value=(0, json.dumps(metadata), ""),
        ), mock.patch(
            "released_codegen_runtime_compatibility.cargo.eprint"
        ) as eprint_mock:
            _verify_runtime_selection("client", Path("/workspace"), candidates)

        summaries = [str(call) for call in eprint_mock.call_args_list]
        self.assertTrue(any("selected from checkout" in line for line in summaries))

    def test_crate_above_the_floor_warns_but_passes(self) -> None:
        """Nudge maintainers to raise the floor when new crates are exercised."""
        from released_codegen_runtime_compatibility.cargo import (
            EXPECTED_RUNTIME_CRATES,
        )

        names = sorted(EXPECTED_RUNTIME_CRATES["client"]) + ["aws-smithy-new-crate"]
        candidates = [
            RuntimeCrate(
                name=name,
                path=Path("/repo/rust-runtime") / name,
                version="0.60.0",
            )
            for name in names
        ]
        metadata = {
            "packages": [
                {
                    "name": crate.name,
                    "version": crate.version,
                    "source": None,
                    "dependencies": [],
                }
                for crate in candidates
            ]
        }
        with mock.patch(
            "released_codegen_runtime_compatibility.cargo.output",
            return_value=(0, json.dumps(metadata), ""),
        ), mock.patch(
            "released_codegen_runtime_compatibility.cargo.LOGGER"
        ) as logger_mock:
            _verify_runtime_selection("client", Path("/workspace"), candidates)

        logger_mock.warning.assert_called_once()
        self.assertIn("aws-smithy-new-crate", str(logger_mock.warning.call_args))

    def test_checkout_path_dependencies_are_ignored(self) -> None:
        """Pass when HEAD moved a crate to a new major that checkout crates use.

        Released generated code keeps its crates.io copy of the old line while a
        patched checkout crate pulls the new line in through a path-only
        dependency, which Cargo reports as the requirement `*`. That path
        requirement says nothing about patch selection and must not be parsed.
        """
        from released_codegen_runtime_compatibility.cargo import (
            EXPECTED_RUNTIME_CRATES,
        )

        candidates = [
            RuntimeCrate(
                name=name,
                path=Path("/repo/rust-runtime") / name,
                version="1.0.0" if name == "aws-smithy-schema" else "0.60.0",
            )
            for name in sorted(EXPECTED_RUNTIME_CRATES["client"])
        ]
        packages = [
            {
                "name": crate.name,
                "version": crate.version,
                "source": None,
                "dependencies": [],
            }
            for crate in candidates
        ]
        runtime = next(p for p in packages if p["name"] == "aws-smithy-runtime")
        runtime["dependencies"].append(
            {
                "name": "aws-smithy-schema",
                "req": "*",
                "path": "/repo/rust-runtime/aws-smithy-schema",
            }
        )
        packages.append(
            {
                "name": "aws-smithy-schema",
                "version": "0.2.1",
                "source": CRATES_IO,
                "dependencies": [],
            }
        )
        packages.append(
            {
                "name": "generated-client",
                "version": "0.0.1",
                "source": None,
                "dependencies": [
                    {
                        "name": "aws-smithy-schema",
                        "source": CRATES_IO,
                        "req": "^0.2.1",
                    }
                ],
            }
        )
        with mock.patch(
            "released_codegen_runtime_compatibility.cargo.output",
            return_value=(0, json.dumps({"packages": packages}), ""),
        ), mock.patch("released_codegen_runtime_compatibility.cargo.eprint"):
            _verify_runtime_selection("client", Path("/workspace"), candidates)
    def test_registry_requirement_is_still_checked_with_path_dependency(self) -> None:
        """A sibling path edge must not hide a crates.io patch-selection bug."""
        candidates = {
            "aws-smithy-schema": RuntimeCrate(
                name="aws-smithy-schema",
                path=Path("/repo/rust-runtime/aws-smithy-schema"),
                version="0.2.2",
            )
        }
        metadata = {
            "packages": [
                {
                    "name": "aws-smithy-runtime",
                    "version": "1.0.0",
                    "source": None,
                    "dependencies": [
                        {
                            "name": "aws-smithy-schema",
                            "source": None,
                            "req": "*",
                            "path": "/repo/rust-runtime/aws-smithy-schema",
                        }
                    ],
                },
                {
                    "name": "aws-smithy-schema",
                    "version": "0.2.1",
                    "source": CRATES_IO,
                    "dependencies": [],
                },
                {
                    "name": "generated-client",
                    "version": "0.0.1",
                    "source": None,
                    "dependencies": [
                        {
                            "name": "aws-smithy-schema",
                            "source": CRATES_IO,
                            "req": "^0.2.1",
                        }
                    ],
                },
            ]
        }
        with mock.patch(
            "released_codegen_runtime_compatibility.cargo.output",
            return_value=(0, json.dumps(metadata), ""),
        ):
            resolved, requirements = _resolved_runtime_packages(
                Path("/workspace"), candidates
            )

        self.assertEqual(
            {"aws-smithy-schema": {("generated-client", "^0.2.1")}},
            requirements,
        )
        problems = _patch_selection_problems(candidates, resolved, requirements)
        self.assertEqual(1, len(problems))
        self.assertIn("aws-smithy-schema 0.2.1 resolved from crates.io", problems[0])


if __name__ == "__main__":
    unittest.main()
