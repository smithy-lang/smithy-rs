#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Export source files to the user's machine, without transferring Git refs/objects.

Preparation updates a CLEAN legacy checkout explicitly. run.py never does so.
"""
import argparse
import json
from pathlib import Path
import shlex
import subprocess
import tarfile
import tempfile
from common import HERE, digest, write_json


def export(repo, archive):
    names = subprocess.check_output(["git", "ls-files", "-z"], cwd=repo).decode().split("\0")
    extra = subprocess.check_output(["git", "ls-files", "--others", "--exclude-standard", "-z"], cwd=repo).decode().split("\0")
    allowed = ("codegen-", "rust-runtime/", "tools/server-event-stream-benchmark/")
    names += [n for n in extra if n.startswith(allowed)]
    names = sorted({n for n in names if n and (repo / n).is_file()})
    meta = {
        "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip(),
        "files": {n: digest(repo / n) for n in names},
        "status": subprocess.check_output(["git", "status", "--porcelain"], cwd=repo, text=True),
    }
    with tempfile.TemporaryDirectory() as tmp:
        write_json(Path(tmp) / "snapshot.json", meta)
        (Path(tmp) / "snapshot.diff").write_bytes(subprocess.check_output(["git", "diff", "HEAD", "--binary"], cwd=repo))
        with tarfile.open(archive, "w:gz") as tar:
            for name in names:
                tar.add(repo / name, arcname=name, recursive=False)
            tar.add(Path(tmp) / "snapshot.json", arcname="snapshot.json")
            tar.add(Path(tmp) / "snapshot.diff", arcname="snapshot.diff")


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--host", default="cloudl")
    p.add_argument("--destination", required=True, help="Absolute NEW directory on the remote machine")
    p.add_argument("--repo", type=Path, default=HERE.parents[1])
    p.add_argument("--update-baseline", action="store_true")
    a = p.parse_args()
    if not a.destination.startswith("/"):
        p.error("--destination must be absolute")
    dest = shlex.quote(a.destination)
    with tempfile.TemporaryDirectory() as tmp:
        archive = Path(tmp) / "source.tar.gz"
        export(a.repo.resolve(), archive)
        # Refuse to replace an existing snapshot. A new directory records a distinct revision.
        subprocess.run(["ssh", a.host, f"test ! -e {dest} && mkdir -p {dest}"], check=True)
        with archive.open("rb") as src:
            subprocess.run(["ssh", a.host, f"tar -xz -C {dest}"], stdin=src, check=True)
    if a.update_baseline:
        subprocess.run(["ssh", a.host, 'test -z "$(git -C ~/smithy-rs-latest status --porcelain)" && git -C ~/smithy-rs-latest pull --ff-only'], check=True)
    print(f"Source snapshot: {a.host}:{a.destination}")
