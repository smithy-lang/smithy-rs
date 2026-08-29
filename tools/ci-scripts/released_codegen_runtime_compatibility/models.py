# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

from dataclasses import dataclass
from pathlib import Path
from typing import Tuple


@dataclass(frozen=True)
class RuntimeCrate:
    """Identify one current runtime crate offered to Cargo as a patch.
    Store only its package name and local source directory.
    """

    name: str
    path: Path


@dataclass(frozen=True)
class PublishedCodegen:
    """Identify one published smithy-rs codegen release from Maven Central.
    Keep its Smithy dependency version aligned with the selected codegen JARs.
    """

    version: str
    smithy_version: str


@dataclass(frozen=True)
class Workspaces:
    """Group representative client and server Cargo workspaces.
    Provide stable labels used in diagnostics and failure aggregation.
    """

    client: Path
    server: Path

    def items(self) -> Tuple[Tuple[str, Path], ...]:
        """Return labeled workspace paths in a deterministic order.
        Keep client and server diagnostics consistent across CI runs.
        """
        return (("client", self.client), ("server", self.server))
