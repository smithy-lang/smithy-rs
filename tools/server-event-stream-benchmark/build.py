#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Build retained generated SDKs. Does not run code generation."""
import argparse
import json
from pathlib import Path
import subprocess
from common import HERE, PROTOCOLS, cargo_env, command, digest, tool_hash, toolchain, validate_generation, write_json


def build(cache):
    provenance = validate_generation(cache)
    sources_before = tool_hash()
    binaries = {}
    locks = {}
    channel = toolchain(provenance)
    compiler = subprocess.check_output(["rustc", f"+{channel}", "-Vv"], env=cargo_env(), text=True)
    for protocol in PROTOCOLS:
        for role in ("legacy", "schema", "client"):
            side = "legacy" if role == "client" else role
            kind = "client" if role == "client" else "server"
            root = cache / "runners" / protocol / role
            src = root / "src"
            src.mkdir(parents=True, exist_ok=True)
            sdk = cache / side / "generated" / f"{protocol}_{kind}" / f"rust-{kind}-codegen"
            runtime = Path(provenance[side]["repo"]) / "rust-runtime"
            manifest = f'''[package]
name = "stream-bench-{role}"
version = "0.0.0"
edition = "2021"
[workspace]
[features]
default = ["{kind}"]
server = []
client = []
allocations = []
[dependencies]
bench_sdk = {{ path = {json.dumps(str(sdk))} }}
aws-smithy-types = {{ path = {json.dumps(str(runtime / "aws-smithy-types"))} }}
tokio = {{ version = "1", features = ["rt-multi-thread", "macros", "net", "io-std", "io-util", "sync", "time"] }}
async-stream = "0.3"
serde_json = "1"
libc = "0.2"
hdrhistogram = "7"
'''
            if kind == "client":
                manifest += f'''aws-smithy-http-client = {{ path = {json.dumps(str(runtime / "aws-smithy-http-client"))}, features = ["hyper-014"] }}
hyper014 = {{ package = "hyper", version = "0.14", features = ["client", "http2", "tcp"] }}
'''
            manifest += "\n[profile.release]\ndebug = 1\n"
            (root / "Cargo.toml").write_text(manifest)
            for file in (f"{kind}.rs", "events.rs", "metrics.rs"):
                (src / ("main.rs" if file == f"{kind}.rs" else file)).write_bytes((HERE / file).read_bytes())
            for mode in ("cpu", "memory", "profile"):
                # Profiling keeps stack frames and debug info; ordinary timing is unaffected.
                env = cargo_env()
                env.pop("CARGO_ENCODED_RUSTFLAGS", None)
                env["RUSTFLAGS"] = "-C force-frame-pointers=yes" if mode == "profile" else ""
                target = root / f"target-{mode}"
                args = ["cargo", f"+{channel}", "build", "--quiet", "--release", "--manifest-path", root / "Cargo.toml", "--target-dir", target]
                if mode == "memory":
                    args += ["--features", "allocations"]
                if (root / "Cargo.lock").exists():
                    args += ["--locked"]
                command(args, env=env, log=root / f"build-{mode}.log")
                binary = target / "release" / f"stream-bench-{role}"
                binaries[str(binary.relative_to(cache))] = digest(binary)
            locks[str((root / "Cargo.lock").relative_to(cache))] = digest(root / "Cargo.lock")
            # Calibrate the requested-heap counter in both process roles. The
            # generated dependency's own tests are not run as part of this tool.
            if protocol == "rest_json" and role in ("legacy", "client"):
                command(["cargo", f"+{channel}", "test", "--quiet", "--release", "--features", "allocations", "--manifest-path", root / "Cargo.toml", "--target-dir", root / "target-memory", "--locked", "--", "--test-threads=1"], env=cargo_env(), log=root / "allocator-test.log")
    validate_generation(cache)
    if sources_before != tool_hash():
        raise SystemExit("Benchmark sources changed during compilation. Run build.py again.")
    write_json(cache / "build.json", {
        "tool_hash": sources_before, "compiler": compiler, "toolchain": channel, "binaries": binaries, "locks": locks,
        "rustflags": {"cpu": "", "memory": "", "profile": "-C force-frame-pointers=yes"},
        "generation_hash": digest(cache / "generation.json"),
    })


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--cache", type=Path, default=HERE / "artifacts/cache")
    a = p.parse_args()
    build(a.cache.resolve())
