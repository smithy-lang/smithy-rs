#!/usr/bin/env python3

# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Detect semver incompatibility between the last codegen released version and
current rust-runtime crates at HEAD.

For example, an old generated SDK may require `aws-smithy-http-server` 0.66.2
and `aws-smithy-json` 0.62.4. If a semver-compatible HTTP server candidate starts
exposing a new incompatible JSON version, compiling that SDK must fail.
"""

import argparse
import logging
from pathlib import Path
from typing import Optional, Sequence

from released_codegen_runtime_compatibility.cargo import CargoVerifier
from released_codegen_runtime_compatibility.codegen import ProtocolSdkGenerator
from released_codegen_runtime_compatibility.commands import (
    configure_logging,
    eprint,
    github_error,
)
from released_codegen_runtime_compatibility.maven import MavenCodegenResolver
from released_codegen_runtime_compatibility.paths import temporary_directory


def parse_args(argv: Optional[Sequence[str]] = None) -> argparse.Namespace:
    """Parse the verification command and attach its handler."""
    parser = argparse.ArgumentParser(
        description="Verify released generated SDKs against runtime crates at HEAD."
    )
    parser.add_argument(
        "--repository-root",
        default=".",
        help="smithy-rs repository root; defaults to the current directory",
    )
    commands = parser.add_subparsers(dest="command", required=True)

    verify = commands.add_parser(
        "verify-semver",
        help="compile released SDKs with semver-eligible runtimes from HEAD",
    )
    verify.add_argument(
        "--codegen-version",
        help="generate temporary SDKs with this published Maven version",
    )
    verify.add_argument(
        "--runtime-root",
        help="runtime workspace to patch from; defaults to <repository-root>/rust-runtime",
    )
    verify.set_defaults(handler=verify_semver)
    return parser.parse_args(argv)


def temporary_prefix(codegen_version: str) -> str:
    """Build a recognizable compatibility-work prefix containing the codegen version."""
    return "smithy-rs-codegen-compat-{}-".format(codegen_version)


def verify_semver(args: argparse.Namespace) -> None:
    """Generate SDKs with published codegen and verify them with current runtimes."""
    repository_root = Path(args.repository_root).resolve()
    runtime_root = (
        Path(args.runtime_root).resolve()
        if args.runtime_root
        else repository_root / "rust-runtime"
    )
    codegen = MavenCodegenResolver().resolve(args.codegen_version)

    with temporary_directory(
        prefix=temporary_prefix(codegen.version)
    ) as temporary_root:
        eprint(
            "generating temporary protocol SDKs with published codegen {}".format(
                codegen.version
            )
        )
        source = ProtocolSdkGenerator(repository_root).generate(
            codegen, temporary_root
        )
        CargoVerifier(runtime_root).verify(
            source, temporary_root / "verification"
        )

    eprint("client and server semver compatibility checks passed")


if __name__ == "__main__":
    configure_logging()
    try:
        args = parse_args()
        args.handler(args)
    except Exception as error:
        logging.getLogger("released-codegen-runtime-compatibility").exception(
            "compatibility command failed"
        )
        github_error(str(error))
        raise SystemExit(1)
