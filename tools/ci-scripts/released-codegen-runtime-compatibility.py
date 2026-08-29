#!/usr/bin/env python3

# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Check released codegen compatibility with runtime crates at HEAD.

The program resolves a published codegen release, generates representative
client and server crates for every supported protocol, offers runtime crates
from the current checkout through Cargo's crates.io patch mechanism, and
compiles the generated workspaces. Cargo only selects patched runtime versions
that satisfy the requirements emitted by the released code generator.
"""

import argparse
import logging
from pathlib import Path
from typing import Optional, Sequence

from released_codegen_runtime_compatibility.commands import (
    configure_logging,
    github_error,
)
from released_codegen_runtime_compatibility.verify_compatibility import (
    verify_released_codegen_runtime_compatibility,
)


def parse_args(argv: Optional[Sequence[str]] = None) -> argparse.Namespace:
    """Parse options for the released-codegen compatibility check."""
    parser = argparse.ArgumentParser(
        description="Check released codegen compatibility with runtime crates at HEAD."
    )
    parser.add_argument(
        "--repository-root",
        default=".",
        help="smithy-rs repository root; defaults to the current directory",
    )
    parser.add_argument(
        "--codegen-version",
        help=(
            "generate temporary SDKs with this published Maven version; defaults "
            "to the latest codegen release published to Maven Central"
        ),
    )
    parser.add_argument(
        "--runtime-root",
        help="runtime workspace to patch from; defaults to <repository-root>/rust-runtime",
    )
    return parser.parse_args(argv)


def main(argv: Optional[Sequence[str]] = None) -> None:
    """Parse CLI options and run the released-codegen compatibility check."""
    configure_logging()
    try:
        args = parse_args(argv)
        verify_released_codegen_runtime_compatibility(
            repository_root=Path(args.repository_root),
            runtime_root=Path(args.runtime_root) if args.runtime_root else None,
            codegen_version=args.codegen_version,
        )
    except Exception as error:
        logging.getLogger("released-codegen-runtime-compatibility").exception(
            "compatibility command failed"
        )
        github_error(str(error))
        raise SystemExit(1)


if __name__ == "__main__":
    main()
