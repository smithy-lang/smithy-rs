# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

import logging
import os
from pathlib import Path
import subprocess
import sys
import time
from typing import Dict, Optional, Sequence, Tuple


LOGGER = logging.getLogger("released-codegen-runtime-compatibility")


def configure_logging() -> None:
    """Configure timestamped logs for local and GitHub Actions execution.
    Allow CODEGEN_RUNTIME_LOG_LEVEL to enable DEBUG diagnostics when needed.
    """
    level_name = os.environ.get("CODEGEN_RUNTIME_LOG_LEVEL", "INFO").upper()
    level = getattr(logging, level_name, logging.INFO)
    logging.basicConfig(
        level=level,
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
        datefmt="%Y-%m-%dT%H:%M:%S",
    )


def eprint(*args: object) -> None:
    """Write an informational diagnostic through the configured logger.
    Retain this small helper so call sites stay concise and consistently formatted.
    """
    LOGGER.info(" ".join(str(argument) for argument in args))


def github_error(message: str) -> None:
    """Emit a GitHub Actions error annotation for the final failure reason.
    Escape workflow-command control characters while retaining full logs above it.
    """
    escaped = (
        message.replace("%", "%25")
        .replace("\r", "%0D")
        .replace("\n", "%0A")
    )
    print(
        "::error title=Codegen runtime compatibility failed::{}".format(escaped),
        file=sys.stderr,
    )


def run(
    command: Sequence[object],
    cwd: Path,
    check: bool = True,
    env: Optional[Dict[str, str]] = None,
) -> subprocess.CompletedProcess:
    """Run a streaming command and log its directory, duration, and exit status.
    Raise after logging when the caller requires a successful command result.
    """
    rendered = [str(part) for part in command]
    display = " ".join(rendered)
    LOGGER.info("starting `%s` in `%s`", display, cwd)
    started = time.monotonic()
    result = subprocess.run(
        rendered,
        cwd=str(cwd),
        check=False,
        env=env,
    )
    duration = time.monotonic() - started
    if result.returncode == 0:
        LOGGER.info("finished `%s` in %.1fs", display, duration)
    else:
        LOGGER.error(
            "command `%s` failed with exit code %s after %.1fs",
            display,
            result.returncode,
            duration,
        )
    if check and result.returncode != 0:
        raise subprocess.CalledProcessError(result.returncode, rendered)
    return result


def output(
    command: Sequence[object], cwd: Path, check: bool = True
) -> Tuple[int, str, str]:
    """Run a captured command and log its directory, duration, and exit status.
    Return decoded streams or raise with both streams when requested.
    """
    rendered = [str(part) for part in command]
    display = " ".join(rendered)
    LOGGER.debug("starting captured `%s` in `%s`", display, cwd)
    started = time.monotonic()
    result = subprocess.run(
        rendered,
        cwd=str(cwd),
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        universal_newlines=True,
    )
    duration = time.monotonic() - started
    if result.returncode == 0:
        LOGGER.debug("finished captured `%s` in %.1fs", display, duration)
    else:
        LOGGER.error(
            "captured command `%s` failed with exit code %s after %.1fs",
            display,
            result.returncode,
            duration,
        )
    if check and result.returncode != 0:
        raise RuntimeError(
            "failed to run `{}`:\n{}\n{}".format(
                display, result.stdout, result.stderr
            )
        )
    return result.returncode, result.stdout.strip(), result.stderr.strip()
