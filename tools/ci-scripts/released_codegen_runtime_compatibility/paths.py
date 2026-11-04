# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Filesystem helpers that behave identically on POSIX and Windows.

Windows rejects paths longer than 260 characters unless they are given in the
extended-length form. Generated SDK trees nest deeply enough to cross that limit
inside a temporary directory, which fails the copy and then fails the cleanup
that follows it. Every bulk copy and delete in this package goes through these
helpers so the compatibility check runs the same way on a developer's Windows
machine as it does on CI.
"""

import contextlib
import os
from pathlib import Path
import shutil
import tempfile
from typing import Iterator, Union


WINDOWS = os.name == "nt"
EXTENDED_PREFIX = "\\\\?\\"
UNC_EXTENDED_PREFIX = "\\\\?\\UNC\\"

PathLike = Union[str, Path]


def long_path(path: PathLike) -> str:
    """Render one path in the form Windows accepts past its 260 character limit.
    Return the ordinary absolute path elsewhere because no other system needs it.
    """
    absolute = str(Path(path).resolve())
    if not WINDOWS or absolute.startswith(EXTENDED_PREFIX):
        return absolute
    if absolute.startswith("\\\\"):
        # A UNC share becomes \\?\UNC\server\share rather than keeping both leading slashes.
        return UNC_EXTENDED_PREFIX + absolute[2:]
    return EXTENDED_PREFIX + absolute


def exists(path: PathLike) -> bool:
    """Report whether one path exists even when it is too long for Windows.
    Use the extended-length form so deep generated trees stay observable.
    """
    return os.path.exists(long_path(path))


def copy_file(source: PathLike, destination: PathLike) -> None:
    """Copy one file, preserving metadata, without tripping the Windows limit.
    Keep the same call shape as `shutil.copy2` for readability at the call sites.
    """
    shutil.copy2(long_path(source), long_path(destination))


def copy_tree(source: PathLike, destination: PathLike) -> None:
    """Copy one directory tree without tripping the Windows path length limit.
    Require the destination to be absent, matching `shutil.copytree` semantics.
    """
    shutil.copytree(long_path(source), long_path(destination))


def remove_tree(path: PathLike) -> None:
    """Delete one directory tree that may contain paths Windows cannot address.
    Ignore an absent tree so callers can clean up unconditionally.
    """
    if exists(path):
        shutil.rmtree(long_path(path))


@contextlib.contextmanager
def temporary_directory(prefix: str) -> Iterator[Path]:
    """Provide a work directory that is removed even when it holds long paths.
    Avoid `tempfile.TemporaryDirectory` because its cleanup cannot delete them.
    """
    directory = Path(tempfile.mkdtemp(prefix=prefix))
    try:
        yield directory
    finally:
        remove_tree(directory)


def gradle_wrapper(repository_root: PathLike) -> Path:
    """Select the Gradle wrapper this platform can actually execute.
    Windows needs the batch wrapper because the extensionless script is a shell script.
    """
    name = "gradlew.bat" if WINDOWS else "gradlew"
    return Path(repository_root) / name
