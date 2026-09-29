# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

import json
import os
from pathlib import Path
import re
from typing import Dict, List, Optional, Sequence, Set, Tuple

from .commands import LOGGER, eprint, output, run
from .models import RuntimeCrate, Workspaces
from .paths import copy_tree


CRATES_IO_SOURCE_PREFIX = "registry+"

# Runtime crates the protocol model must keep in each resolved dependency
# graph. If one disappears, the compatibility model no longer exercises that
# crate and this check would silently stop guarding it against semver breaks.
EXPECTED_RUNTIME_CRATES: Dict[str, Set[str]] = {
    "client": {
        "aws-smithy-async",
        "aws-smithy-cbor",
        "aws-smithy-eventstream",
        "aws-smithy-http",
        "aws-smithy-http-client",
        "aws-smithy-json",
        "aws-smithy-observability",
        "aws-smithy-protocol-test",
        "aws-smithy-query",
        "aws-smithy-runtime",
        "aws-smithy-runtime-api",
        "aws-smithy-runtime-api-macros",
        "aws-smithy-schema",
        "aws-smithy-types",
        "aws-smithy-xml",
    },
    "server": {
        "aws-smithy-async",
        "aws-smithy-cbor",
        "aws-smithy-eventstream",
        "aws-smithy-http",
        "aws-smithy-http-server",
        "aws-smithy-json",
        "aws-smithy-legacy-http",
        "aws-smithy-legacy-http-server",
        "aws-smithy-runtime-api",
        "aws-smithy-runtime-api-macros",
        "aws-smithy-schema",
        "aws-smithy-types",
        "aws-smithy-xml",
    },
}


def patch_generated_sdks_with_current_runtimes(
    generated_sdks: Workspaces,
    runtime_crates: Sequence[RuntimeCrate],
    destination_root: Path,
) -> Workspaces:
    """Copy generated SDKs and offer current runtimes through Cargo patches.

    Cargo selects a patched runtime only when its current version satisfies the
    dependency requirement emitted by released codegen. Generated registry
    requirements remain intact, and lockfiles are removed before resolution.
    """
    patched_sdks = _copy_workspaces(generated_sdks, destination_root)
    for label, workspace in patched_sdks.items():
        _assert_registry_runtime_dependencies(workspace)
        _append_runtime_patches(workspace, runtime_crates)
        eprint(
            "patched {} workspace with {} current runtime crates".format(
                label, len(runtime_crates)
            )
        )
    return patched_sdks


def compile_generated_sdks(
    generated_sdks: Workspaces, runtime_crates: Sequence[RuntimeCrate]
) -> None:
    """Compile client and server SDK workspaces and report all failures together."""
    failures = []
    for label, workspace in generated_sdks.items():
        eprint("compiling generated {} SDK workspace".format(label))
        try:
            _check_workspace(label, workspace)
            _verify_runtime_selection(label, workspace, runtime_crates)
        except RuntimeError as error:
            failures.append(str(error))
    if failures:
        raise RuntimeError("\n".join(failures))
    eprint("released codegen compatibility checks passed")


def discover_runtime_crates(runtime_root: Path) -> Sequence[RuntimeCrate]:
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
                version=package["version"],
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


def _verify_runtime_selection(
    label: str, workspace: Path, runtime_crates: Sequence[RuntimeCrate]
) -> None:
    """Verify where each runtime crate resolved from after compilation.

    Fail when a runtime crate resolved from crates.io for a requirement the
    checkout candidate also satisfies (the patch should have won), or when an
    expected runtime crate is missing from the resolved graph (the model no
    longer exercises it). Print a selected/unused summary instead of relying
    on suppressed Cargo warnings.
    """
    candidates = {crate.name: crate for crate in runtime_crates}
    resolved, requirements = _resolved_runtime_packages(workspace, candidates)

    problems = _patch_selection_problems(candidates, resolved, requirements)
    missing = EXPECTED_RUNTIME_CRATES[label] - set(resolved)
    if missing:
        problems.append(
            "{} workspace no longer exercises expected runtime crates: {}".format(
                label, ", ".join(sorted(missing))
            )
        )
    unexpected = set(resolved) - EXPECTED_RUNTIME_CRATES[label]
    if unexpected:
        # Warn rather than fail so newly exercised crates don't block CI, but
        # nudge maintainers to raise the floor and lock in the added coverage.
        LOGGER.warning(
            "%s workspace exercises runtime crates missing from "
            "EXPECTED_RUNTIME_CRATES; add them to keep this coverage guarded: %s",
            label,
            ", ".join(sorted(unexpected)),
        )

    selected = sorted(
        name
        for name, entries in resolved.items()
        if any(source is None for _, source in entries)
    )
    from_registry = sorted(
        name
        for name, entries in resolved.items()
        if any(source is not None for _, source in entries)
    )
    unused = sorted(set(candidates) - set(resolved))
    eprint(
        "{} runtime crate selection: {} from checkout, {} from crates.io, {} unused candidates".format(
            label, len(selected), len(from_registry), len(unused)
        )
    )
    eprint("  selected from checkout: {}".format(", ".join(selected) or "none"))
    eprint("  resolved from crates.io: {}".format(", ".join(from_registry) or "none"))
    eprint("  unused patch candidates: {}".format(", ".join(unused) or "none"))

    if problems:
        raise RuntimeError(
            "{} runtime selection verification failed:\n{}".format(
                label, "\n".join(problems)
            )
        )


def _resolved_runtime_packages(
    workspace: Path, candidates: Dict[str, RuntimeCrate]
) -> Tuple[
    Dict[str, Set[Tuple[str, Optional[str]]]], Dict[str, Set[Tuple[str, str]]]
]:
    """Read resolved runtime versions/sources and the requirements on them."""
    _, metadata_json, _ = output(
        [
            "cargo",
            "metadata",
            "--format-version",
            "1",
            "--all-features",
        ],
        workspace,
    )
    metadata = json.loads(metadata_json)
    resolved: Dict[str, Set[Tuple[str, Optional[str]]]] = {}
    requirements: Dict[str, Set[Tuple[str, str]]] = {}
    for package in metadata["packages"]:
        name = package["name"]
        if name in candidates:
            resolved.setdefault(name, set()).add(
                (package["version"], package["source"])
            )
        for dependency in package["dependencies"]:
            if dependency["name"] in candidates:
                requirements.setdefault(dependency["name"], set()).add(
                    (name, dependency["req"])
                )
    return resolved, requirements


def _patch_selection_problems(
    candidates: Dict[str, RuntimeCrate],
    resolved: Dict[str, Set[Tuple[str, Optional[str]]]],
    requirements: Dict[str, Set[Tuple[str, str]]],
) -> List[str]:
    """Report crates.io selections the checkout candidate should have won."""
    problems = []
    for name, entries in sorted(resolved.items()):
        candidate = candidates[name]
        registry_versions = sorted(
            version
            for version, source in entries
            if source is not None and source.startswith(CRATES_IO_SOURCE_PREFIX)
        )
        for registry_version in registry_versions:
            for dependent, requirement in sorted(requirements.get(name, ())):
                if _satisfies_requirement(
                    candidate.version, requirement
                ) and _satisfies_requirement(registry_version, requirement):
                    problems.append(
                        "{} {} resolved from crates.io for `{}` requirement `{}` "
                        "even though checkout candidate {} satisfies it".format(
                            name,
                            registry_version,
                            dependent,
                            requirement,
                            candidate.version,
                        )
                    )
    return problems


def _satisfies_requirement(version: str, requirement: str) -> bool:
    """Apply Cargo's caret and exact requirement rules to one clause list."""
    return all(
        _satisfies_clause(version, clause.strip())
        for clause in requirement.split(",")
    )


def _satisfies_clause(version: str, clause: str) -> bool:
    parsed_version = _parse_version(version)
    if clause.startswith("="):
        required = _parse_version(clause[1:].strip())
        return parsed_version[: len(required)] == required
    if clause.startswith("^"):
        clause = clause[1:].strip()
    if not re.match(r"^\d+(\.\d+){0,2}$", clause):
        # Codegen only emits caret and exact requirements; surface anything
        # else instead of guessing at its semantics.
        raise RuntimeError("unsupported version requirement `{}`".format(clause))
    required = _parse_version(clause)
    padded = required + (0,) * (3 - len(required))
    if parsed_version < padded:
        return False
    # Cargo caret semantics: stay within the leftmost non-zero component.
    for index, part in enumerate(required):
        if part != 0:
            return parsed_version[:index + 1] == required[:index + 1]
    return parsed_version[: len(required)] == required


def _parse_version(text: str) -> Tuple[int, ...]:
    release = text.split("-")[0].split("+")[0]
    return tuple(int(part) for part in release.split("."))
