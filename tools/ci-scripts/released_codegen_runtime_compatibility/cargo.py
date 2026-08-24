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


class CargoVerifier:
    """Verify released generated SDKs against current runtime crate candidates.
    Let Cargo enforce generated semver requirements through crates.io patches.
    """

    def __init__(self, runtime_root: Path) -> None:
        """Store the current runtime Cargo workspace used for candidate patches.
        Discover package names and paths from this workspace during verification.
        """
        self.runtime_root = runtime_root

    def verify(self, source: Workspaces, destination_root: Path) -> None:
        """Copy, patch, and compile client and server compatibility workspaces.
        Check both workspaces before reporting all accumulated failures.
        """
        workspaces = self._copy_workspaces(source, destination_root)
        runtime_crates = self._discover_runtime_crates()
        failures = []
        for label, workspace in workspaces.items():
            self._assert_registry_runtime_dependencies(workspace)
            self._append_runtime_patches(workspace, runtime_crates)
            eprint(
                "checking {} workspace with {} candidate runtime crate patches".format(
                    label, len(runtime_crates)
                )
            )
            try:
                self._check_workspace(label, workspace)
            except RuntimeError as error:
                failures.append(str(error))
        if failures:
            raise RuntimeError("\n".join(failures))

    def _discover_runtime_crates(self) -> Sequence[RuntimeCrate]:
        """Read runtime workspace packages through structured Cargo metadata.
        Return publishable AWS crates as names and local paths for patching.
        """
        manifest_path = self.runtime_root / "Cargo.toml"
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
            self.runtime_root,
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
                "no publishable runtime crates found under {}".format(self.runtime_root)
            )
        return crates

    def _copy_workspaces(self, source: Workspaces, destination_root: Path) -> Workspaces:
        """Copy source workspaces to an isolated verification directory.
        Keep Cargo patches, lockfiles, and build outputs out of generated SDKs.
        """
        copies = {}
        for label, workspace in source.items():
            destination = destination_root / label
            copy_tree(workspace, destination)
            copies[label] = destination
        return Workspaces(client=copies["client"], server=copies["server"])

    def _assert_registry_runtime_dependencies(self, workspace: Path) -> None:
        """Ensure generated SDK manifests retained released registry requirements.
        Reject local runtime paths because they bypass Cargo semver selection.
        """
        local_runtime_paths = []
        for manifest in workspace.rglob("Cargo.toml"):
            if manifest == workspace / "Cargo.toml":
                continue
            for line in manifest.read_text().splitlines():
                if re.match(
                    r'^\s*path\s*=\s*".*(?:rust-runtime|aws/rust-runtime)', line
                ):
                    local_runtime_paths.append(
                        "{}: {}".format(manifest, line.strip())
                    )
        if local_runtime_paths:
            raise RuntimeError(
                "generated SDK retained local runtime dependencies:\n{}".format(
                    "\n".join(local_runtime_paths)
                )
            )

    def _append_runtime_patches(
        self, workspace: Path, runtime_crates: Sequence[RuntimeCrate]
    ) -> None:
        """Offer current runtime path crates through the crates.io patch table.
        Remove lockfiles so Cargo resolves them against generated SDK requirements.
        """
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

    def _check_workspace(self, label: str, workspace: Path) -> None:
        """Compile every target and feature in one patched compatibility workspace.
        Print duplicate dependency versions and raise a labeled error on failure.
        """
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
                "{} compatibility check failed; duplicate versions follow:".format(
                    label
                )
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
