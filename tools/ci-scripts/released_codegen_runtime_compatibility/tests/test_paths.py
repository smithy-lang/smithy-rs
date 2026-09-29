# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

import os
from pathlib import Path
import tempfile
import unittest

from released_codegen_runtime_compatibility import paths


class LongPathTest(unittest.TestCase):
    def test_extended_form_is_windows_only(self) -> None:
        """Add the extended-length prefix only where Windows requires it.
        Leave POSIX paths untouched so nothing changes on CI.
        """
        rendered = paths.long_path(Path.cwd())
        if paths.WINDOWS:
            self.assertTrue(rendered.startswith(paths.EXTENDED_PREFIX))
        else:
            self.assertFalse(rendered.startswith(paths.EXTENDED_PREFIX))
            self.assertEqual(str(Path.cwd().resolve()), rendered)

    def test_extended_form_is_not_applied_twice(self) -> None:
        """Keep an already extended path unchanged when it is rendered again.
        Guard against nesting the prefix through repeated helper calls.
        """
        once = paths.long_path(Path.cwd())
        self.assertEqual(once, paths.long_path(once))

    def test_relative_paths_become_absolute(self) -> None:
        """Resolve relative inputs because the extended form requires a full path.
        Keep call sites free to pass whatever `Path` they already hold.
        """
        self.assertEqual(paths.long_path(Path.cwd()), paths.long_path("."))


class DeepTreeTest(unittest.TestCase):
    """Exercise the copy and delete helpers on a tree Windows cannot address plainly.
    Nest far past 260 characters so the extended-length form is what makes it work.
    """

    @staticmethod
    def deep_relative_path() -> Path:
        """Build a relative path long enough to exceed the Windows limit.
        Mirror the nesting depth of a generated SDK's protocol serialization tree.
        """
        return Path(*["generated_sdk_directory_segment"] * 10) / "shape.rs"

    def test_copy_and_remove_survive_long_paths(self) -> None:
        """Copy and delete a deeply nested tree without hitting a path limit.
        This is the failure that stopped the check from running on Windows.
        """
        # Hold the fixture in the package's own work directory: `tempfile` cannot delete a tree
        # this deep on Windows, which is the whole reason these helpers exist.
        with paths.temporary_directory("codegen-compat-test-") as root:
            source = root / "source"
            relative = self.deep_relative_path()
            leaf = source / relative
            os.makedirs(paths.long_path(leaf.parent), exist_ok=True)
            with open(paths.long_path(leaf), "w") as handle:
                handle.write("generated")

            destination = root / "destination"
            paths.copy_tree(source, destination)

            copied = destination / relative
            self.assertTrue(paths.exists(copied))
            with open(paths.long_path(copied)) as handle:
                self.assertEqual("generated", handle.read())

            paths.remove_tree(destination)
            self.assertFalse(paths.exists(destination))

    def test_remove_tree_tolerates_a_missing_tree(self) -> None:
        """Ignore an absent directory so callers can clean up unconditionally.
        Keep teardown from masking the failure that actually mattered.
        """
        with tempfile.TemporaryDirectory() as temp:
            paths.remove_tree(Path(temp) / "never-created")


class TemporaryDirectoryTest(unittest.TestCase):
    def test_work_directory_is_removed_even_when_deeply_nested(self) -> None:
        """Remove the work directory even when it holds paths Windows cannot address.
        `tempfile.TemporaryDirectory` fails this cleanup, which is why it is not used.
        """
        with paths.temporary_directory("codegen-compat-test-") as directory:
            leaf = directory / DeepTreeTest.deep_relative_path()
            os.makedirs(paths.long_path(leaf.parent), exist_ok=True)
            with open(paths.long_path(leaf), "w") as handle:
                handle.write("generated")
            self.assertTrue(paths.exists(leaf))
        self.assertFalse(paths.exists(directory))


class GradleWrapperTest(unittest.TestCase):
    def test_wrapper_matches_the_running_platform(self) -> None:
        """Select the wrapper this platform can execute through `subprocess`.
        Windows cannot exec the extensionless shell script the repository ships.
        """
        wrapper = paths.gradle_wrapper(Path("/repo"))
        expected = "gradlew.bat" if paths.WINDOWS else "gradlew"
        self.assertEqual(expected, wrapper.name)
        self.assertEqual(Path("/repo"), wrapper.parent)


if __name__ == "__main__":
    unittest.main()
