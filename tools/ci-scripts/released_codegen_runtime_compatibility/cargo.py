# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

import json
import os
from pathlib import Path
import re
from typing import List, Sequence

from .commands import eprint, output, run
from .models import RuntimeCrate, Workspaces
from .paths import copy_tree


def patch_generated_sdks_with_current_runtimes(
    generated_sdks: Workspaces,
    runtime_root: Path,
    destination_root: Path,
) -> Workspaces:
    """Copy generated SDKs and offer current runtimes through Cargo patches.

    Cargo selects a patched runtime only when its current version satisfies the
    dependency requirement emitted by released codegen. Generated registry
    requirements remain intact, and lockfiles are removed before resolution.
    """
    patched_sdks = _copy_workspaces(generated_sdks, destination_root)
    runtime_crates = _discover_runtime_crates(runtime_root)
    for label, workspace in patched_sdks.items():
        _assert_registry_runtime_dependencies(workspace)
        _append_runtime_patches(workspace, runtime_crates)
        eprint(
            "patched {} workspace with {} current runtime crates".format(
                label, len(runtime_crates)
            )
        )
    return patched_sdks


def compile_generated_sdks(generated_sdks: Workspaces) -> None:
    """Compile client and server SDK workspaces and report all failures together."""
    failures = []
    for label, workspace in generated_sdks.items():
        eprint("compiling generated {} SDK workspace".format(label))
        try:
            _check_workspace(label, workspace)
        except RuntimeError as error:
            failures.append(str(error))
    if failures:
        raise RuntimeError("\n".join(failures))
    eprint("released codegen compatibility checks passed")


def _discover_runtime_crates(runtime_root: Path) -> Sequence[RuntimeCrate]:
    """Read publishable AWS runtime packages through structured Cargo metadata."""
    manifest_path = runtime_root / "Cargo.toml"
    _, metadata_json, _ = output(
        [
            "cargo",
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
            manifest_path,
        ],
        runtime_root,
    )
    metadata = json.loads(metadata_json)
    crates: List[RuntimeCrate] = []
    for package in metadata["packages"]:
        if not package["name"].startswith("aws-"):
            continue
        # Cargo metadata represents `publish = false` as an empty registry list. Do not add
        # unpublished crates to `[patch.crates-io]`: that would make a crate available to this
        # test even though a customer's crates.io dependency graph could never resolve it.
        # Patched runtime crates can still use unpublished helpers through local path
        # dependencies.
        if package.get("publish") == []:
            continue
        crates.append(
            RuntimeCrate(
                name=package["name"],
                path=Path(package["manifest_path"]).resolve().parent,
            )
        )
    crates.sort(key=lambda crate: crate.name)
    if not crates:
        raise RuntimeError(
            "no publishable runtime crates found under {}".format(runtime_root)
        )
    return crates


def _copy_workspaces(source: Workspaces, destination_root: Path) -> Workspaces:
    """Copy source workspaces so patches and build outputs remain isolated."""
    copies = {}
    for label, workspace in source.items():
        destination = destination_root / label
        copy_tree(workspace, destination)
        copies[label] = destination
    return Workspaces(client=copies["client"], server=copies["server"])


def _assert_registry_runtime_dependencies(workspace: Path) -> None:
    """Reject local runtime paths because they bypass Cargo semver selection."""
    local_runtime_paths = []
    for manifest in workspace.rglob("Cargo.toml"):
        if manifest == workspace / "Cargo.toml":
            continue
        for line in manifest.read_text().splitlines():
            if re.match(
                r'^\s*path\s*=\s*".*(?:rust-runtime|aws/rust-runtime)', line
            ):
                local_runtime_paths.append("{}: {}".format(manifest, line.strip()))
    if local_runtime_paths:
        raise RuntimeError(
            "generated SDK retained local runtime dependencies:\n{}".format(
                "\n".join(local_runtime_paths)
            )
        )


def _append_runtime_patches(
    workspace: Path, runtime_crates: Sequence[RuntimeCrate]
) -> None:
    """Add current runtime paths to the crates.io patch table and remove lockfiles."""
    manifest = workspace / "Cargo.toml"
    contents = manifest.read_text()
    if re.search(r"(?m)^\[patch\.crates-io\]\s*$", contents):
        raise RuntimeError(
            "{} already has a [patch.crates-io] section".format(manifest)
        )

    patch_lines = ["", "# Candidate runtime release under test.", "[patch.crates-io]"]
    for crate in runtime_crates:
        # Cargo reads the version from this path and selects it only when it satisfies the
        # requirement preserved in the generated SDK. An incompatible major remains unused.
        # JSON string escaping is also valid for a TOML basic string.
        patch_lines.append(
            "{} = {{ path = {} }}".format(crate.name, json.dumps(str(crate.path)))
        )
    manifest.write_text(contents.rstrip() + "\n" + "\n".join(patch_lines) + "\n")

    for lockfile in workspace.rglob("Cargo.lock"):
        lockfile.unlink()


def _check_workspace(label: str, workspace: Path) -> None:
    """Compile every target and feature in one patched compatibility workspace."""
    cargo_env = dict(os.environ)
    # Old generated code may legitimately use APIs that are now deprecated.
    cargo_env.pop("RUSTFLAGS", None)
    result = run(
        [
            "cargo",
            "check",
            "--workspace",
            "--all-features",
            "--all-targets",
            "--quiet",
        ],
        workspace,
        check=False,
        env=cargo_env,
    )
    if result.returncode != 0:
        eprint(
            "{} compatibility check failed; duplicate versions follow:".format(label)
        )
        run(
            ["cargo", "tree", "--duplicates"],
            workspace,
            check=False,
            env=cargo_env,
        )
        raise RuntimeError(
            "{} SDK does not compile with semver-eligible runtime crates from HEAD".format(
                label
            )
        )
