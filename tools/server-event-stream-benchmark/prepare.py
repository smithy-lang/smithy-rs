#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Generate both servers and ONE common client per protocol. Never updates Git."""
import argparse
import json
import os
from pathlib import Path
import subprocess
from common import HERE, PROTOCOLS, cargo_env, command, digest, fingerprint, generation_hash, validate_generation, write_json


def prepare(cache, baseline, current):
    if (cache / "generation.json").exists():
        validate_generation(cache)
        print(f"Reusing {cache}")
        return
    cache.mkdir(parents=True, exist_ok=True)
    provenance = {"generation_hash": generation_hash(), "generated_hashes": {}}
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
    p.add_argument("--baseline", type=Path, default=Path.home() / "smithy-rs-latest")
    p.add_argument("--current", type=Path, default=HERE.parents[1])
    a = p.parse_args()
    prepare(a.cache.resolve(), a.baseline.resolve(), a.current.resolve())
