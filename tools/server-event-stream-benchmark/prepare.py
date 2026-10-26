#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Generate both servers and ONE common client per protocol. Never updates Git."""
import argparse
import json
import os
from pathlib import Path
import subprocess
from common import DEFAULT_LEGACY_RELEASE, HERE, PROTOCOLS, cargo_env, command, digest, fingerprint, generation_hash, validate_generation, write_json


def verify_legacy_release(baseline, release):
    def git(*args):
        return subprocess.check_output(["git", *args], cwd=baseline, text=True, stderr=subprocess.DEVNULL).strip()

    try:
        head = git("rev-parse", "HEAD")
    except (subprocess.CalledProcessError, OSError):
        raise SystemExit(f"Legacy baseline {baseline} is not a Git checkout")
    try:
        tagged = git("rev-parse", f"{release}^{{commit}}")
    except subprocess.CalledProcessError:
        raise SystemExit(f"Legacy baseline {baseline} has no tag {release}; fetch tags there or pass --legacy-release")
    if head != tagged:
        raise SystemExit(f"Legacy baseline HEAD {head[:12]} is not {release} ({tagged[:12]}); check out that release or pass --legacy-release")
    if git("status", "--porcelain"):
        raise SystemExit(f"Legacy baseline {baseline} has uncommitted changes; the pinned release must be clean")


def prepare(cache, baseline, current, legacy_release, comparison="same-revision"):
    if comparison == "same-revision" and baseline != current:
        raise SystemExit("same-revision comparison must generate both servers from --current")
    if (cache / "generation.json").exists():
        data = validate_generation(cache)
        if data.get("comparison") != comparison:
            raise SystemExit("Cache has a different comparison design. Use a new cache.")
        if comparison == "release" and data.get("legacy_release") != legacy_release:
            raise SystemExit(f"Cache was prepared with legacy release {data.get('legacy_release')!r}, not {legacy_release!r}. Use a new cache.")
        print(f"Reusing {cache}")
        return
    if comparison == "release":
        verify_legacy_release(baseline, legacy_release)
    cache.mkdir(parents=True, exist_ok=True)
    provenance = {
        "generation_hash": generation_hash(), "comparison": comparison,
        "legacy_release": legacy_release if comparison == "release" else None,
        "generated_hashes": {},
    }
    for side, repo in (("legacy", baseline), ("schema", current)):
        dest = cache / side
        dest.mkdir(exist_ok=True)
        before = fingerprint(repo)
        projections = {}
        for protocol, trait in PROTOCOLS.items():
            model = dest / f"{protocol}.smithy"
            model.write_text((HERE / "model.smithy").read_text().replace("// PROTOCOL", trait))
            for kind in (["server", "client"] if side == "legacy" else ["server"]):
                name = f"{protocol}_{kind}"
                settings = {
                    "service": "benchmark.eventstream#BenchmarkService",
                    "module": "bench_sdk",
                    "moduleVersion": "0.0.1",
                    "moduleAuthors": ["Smithy event stream benchmark"],
                    "moduleDescription": "Generated event stream benchmark SDK",
                    "runtimeConfig": {"relativePath": str(repo / "rust-runtime")},
                    "codegen": {"http-1x": True},
                }
                if kind == "server":
                    settings["codegen"]["alwaysSendEventStreamInitialResponse"] = protocol == "rpc_v2_cbor"
                    settings_file = repo / "codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/ServerRustSettings.kt"
                    if "schemaSerde" in settings_file.read_text():
                        settings["codegen"]["schemaSerde"] = side == "schema"
                    elif side == "schema":
                        raise SystemExit("Current checkout does not support schemaSerde")
                projections[name] = {"imports": [str(model)], "plugins": {f"rust-{kind}-codegen": settings}}
        for kind in (["server", "client"] if side == "legacy" else ["server"]):
            selected = {k: v for k, v in projections.items() if k.endswith(kind)}
            config = dest / f"smithy-build-{kind}.json"
            write_json(config, {"version": "1.0", "projections": selected})
            env = {**cargo_env(), "EVENT_STREAM_BENCH_CONFIG": str(config), "EVENT_STREAM_BENCH_OUTPUT": str(dest / "generated")}
            command([repo / "gradlew", "-I", HERE / "codegen.init.gradle", f":codegen-{kind}:generateEventStreamBenchmark", "--quiet"], cwd=repo, env=env, log=dest / f"generation-{kind}.log")
        if fingerprint(repo) != before:
            raise SystemExit(f"{side} sources changed during code generation")
        try:
            revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True, stderr=subprocess.DEVNULL).strip()
            diff = subprocess.check_output(["git", "diff", "HEAD", "--binary"], cwd=repo)
            (dest / "source.diff").write_bytes(diff)
        except subprocess.CalledProcessError:
            revision = json.loads((repo / "snapshot.json").read_text())["revision"]
        provenance[side] = {"repo": str(repo), "source_hash": before, "revision": revision}
        for name in projections:
            kind = name.rsplit("_", 1)[1]
            crate = dest / "generated" / name / f"rust-{kind}-codegen"
            if not (crate / "Cargo.toml").exists():
                raise SystemExit(f"Missing generated crate: {crate}")
            for p in crate.rglob("*"):
                if p.is_file():
                    provenance["generated_hashes"][str(p.relative_to(cache))] = digest(p)
    write_json(cache / "generation.json", provenance)


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--cache", type=Path, default=HERE / "artifacts/cache")
    p.add_argument("--comparison", choices=["same-revision", "release"], default="same-revision")
    p.add_argument("--baseline", type=Path, help="Separate checkout for --comparison release")
    p.add_argument("--current", type=Path, default=HERE.parents[1])
    p.add_argument("--legacy-release", default=DEFAULT_LEGACY_RELEASE, help="Release tag the legacy baseline must be checked out at")
    a = p.parse_args()
    if a.baseline and a.comparison != "release":
        p.error("--baseline requires --comparison release")
    baseline = a.current if a.comparison == "same-revision" else (a.baseline or Path.home() / "smithy-rs-latest")
    prepare(a.cache.resolve(), baseline.resolve(), a.current.resolve(), a.legacy_release, a.comparison)
