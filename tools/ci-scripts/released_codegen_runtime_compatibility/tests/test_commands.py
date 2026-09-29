# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

from contextlib import redirect_stderr
import io
import unittest

from released_codegen_runtime_compatibility.commands import github_error


class GithubDiagnosticsTest(unittest.TestCase):
    def test_github_error_escapes_workflow_characters(self) -> None:
        """Render a visible GitHub Actions error annotation for command failures.
        Escape newlines and percent signs so diagnostics cannot break the protocol.
        """
        stderr = io.StringIO()
        with redirect_stderr(stderr):
            github_error("cargo failed 100%\nserver SDK")

        self.assertEqual(
            "::error title=Codegen runtime compatibility failed::"
            "cargo failed 100%25%0Aserver SDK\n",
            stderr.getvalue(),
        )


if __name__ == "__main__":
    unittest.main()
