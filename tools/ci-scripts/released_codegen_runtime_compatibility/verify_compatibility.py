# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Generate SDKs with released codegen and compile them with current runtimes."""

from pathlib import Path
from typing import Optional

from .cargo import (
    compile_generated_sdks,
    patch_generated_sdks_with_current_runtimes,
)
from .codegen import COMPATIBILITY_PROTOCOLS, generate_protocol_sdks
from .maven import resolve_published_codegen
from .paths import temporary_directory


def temporary_prefix(codegen_version: str) -> str:
    """Build a recognizable compatibility-work prefix containing the codegen version."""
    return "smithy-rs-codegen-compat-{}-".format(codegen_version)


def verify_released_codegen_runtime_compatibility(
    repository_root: Path,
    runtime_root: Optional[Path] = None,
    codegen_version: Optional[str] = None,
) -> None:
    """Generate SDKs with published codegen and verify them with current runtimes."""
    repository_root = repository_root.resolve()
    runtime_root = (
        runtime_root.resolve()
        if runtime_root is not None
        else repository_root / "rust-runtime"
    )
    published_codegen = resolve_published_codegen(codegen_version)

    with temporary_directory(
        prefix=temporary_prefix(published_codegen.version)
    ) as work_directory:
        generated_sdks = generate_protocol_sdks(
            published_codegen=published_codegen,
            protocols=COMPATIBILITY_PROTOCOLS,
            repository_root=repository_root,
            destination=work_directory / "generation",
        )
        patched_sdks = patch_generated_sdks_with_current_runtimes(
            generated_sdks=generated_sdks,
            runtime_root=runtime_root,
            destination_root=work_directory / "verification",
        )
        compile_generated_sdks(patched_sdks)
