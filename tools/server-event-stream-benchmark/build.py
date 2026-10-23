#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Build retained generated SDKs. Does not run code generation."""
import argparse
import json
from pathlib import Path
import subprocess
from common import HERE, PROTOCOLS, cargo_env, command, digest, tool_hash, toolchain, validate_generation, write_json


MODES = ("cpu", "memory", "profile", "runtime", "timing", "dial9")
RUSTFLAGS = {"profile": "-C force-frame-pointers=yes",
             "runtime": "--cfg tokio_unstable", "dial9": "--cfg tokio_unstable"}


def build(cache, modes=MODES):
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
runtime-diagnostics = ["tokio/schedule-latency"]
server-timing = ["dep:http", "dep:http-body", "dep:http-body-util", "dep:tower", "dep:pin-project-lite", "dep:bytes"]
dial9 = ["server-timing", "dep:dial9", "dep:dial9-utils", "dep:tracing", "dep:tracing-subscriber"]
[dependencies]
bench_sdk = {{ path = {json.dumps(str(sdk))} }}
aws-smithy-types = {{ path = {json.dumps(str(runtime / "aws-smithy-types"))} }}
tokio = {{ version = "1.54", features = ["rt-multi-thread", "macros", "net", "io-std", "io-util", "sync", "time"] }}
async-stream = "0.3"
serde_json = "1"
libc = "0.2"
hdrhistogram = "7"
http = {{ version = "1", optional = true }}
http-body = {{ version = "1", optional = true }}
http-body-util = {{ version = "0.1", optional = true }}
tower = {{ version = "0.5", features = ["util"], optional = true }}
pin-project-lite = {{ version = "0.2", optional = true }}
bytes = {{ version = "1", optional = true }}
dial9 = {{ version = "=0.5.5", features = ["tokio"], optional = true }}
dial9-utils = {{ version = "=0.5.5", features = ["tracing-layer"], optional = true }}
tracing = {{ version = "0.1", optional = true }}
tracing-subscriber = {{ version = "0.3", default-features = false, features = ["registry"], optional = true }}
'''
            if kind == "client":
                manifest += f'''aws-smithy-http-client = {{ path = {json.dumps(str(runtime / "aws-smithy-http-client"))}, features = ["hyper-014"] }}
hyper014 = {{ package = "hyper", version = "0.14", features = ["client", "http2", "tcp"] }}
'''
            manifest += "\n[profile.release]\ndebug = 1\n"
            manifest_path = root / "Cargo.toml"
            manifest_changed = not manifest_path.exists() or manifest_path.read_text() != manifest
            manifest_path.write_text(manifest)
            for file in (f"{kind}.rs", "events.rs", "metrics.rs", "timing.rs"):
                (src / ("main.rs" if file == f"{kind}.rs" else file)).write_bytes((HERE / file).read_bytes())
            # Resolve the new optional diagnostic dependencies before using
            # --locked, retaining the resolved lockfile with the binaries.
            if manifest_changed and (root / "Cargo.lock").exists():
                command(["cargo", f"+{channel}", "generate-lockfile", "--manifest-path", root / "Cargo.toml"], env=cargo_env(), log=root / "resolve.log")
            role_modes = [m for m in modes if kind == "server" or m not in ("timing", "dial9")]
            for mode in role_modes:
                # Profiling keeps stack frames and debug info; ordinary timing is unaffected.
                env = cargo_env()
                env.pop("CARGO_ENCODED_RUSTFLAGS", None)
                env["RUSTFLAGS"] = RUSTFLAGS.get(mode, "")
                target = root / f"target-{mode}"
                args = ["cargo", f"+{channel}", "build", "--quiet", "--release", "--manifest-path", root / "Cargo.toml", "--target-dir", target]
                if mode == "memory":
                    args += ["--features", "allocations"]
                if mode == "runtime":
                    args += ["--features", "runtime-diagnostics"]
                if mode == "timing":
                    args += ["--features", "server-timing"]
                if mode == "dial9":
                    args += ["--features", "dial9"]
                if (root / "Cargo.lock").exists():
                    args += ["--locked"]
                command(args, env=env, log=root / f"build-{mode}.log")
                binary = target / "release" / f"stream-bench-{role}"
                binaries[str(binary.relative_to(cache))] = digest(binary)
            locks[str((root / "Cargo.lock").relative_to(cache))] = digest(root / "Cargo.lock")
            # Calibrate the requested-heap counter in both process roles. The
            # generated dependency's own tests are not run as part of this tool.
            if protocol == "rest_json" and role in ("legacy", "client"):
                if "memory" in modes:
                    command(["cargo", f"+{channel}", "test", "--quiet", "--release", "--features", "allocations", "--manifest-path", root / "Cargo.toml", "--target-dir", root / "target-memory", "--locked", "--", "--test-threads=1"], env=cargo_env(), log=root / "allocator-test.log")
                if "runtime" in modes:
                    env = cargo_env()
                    env.pop("CARGO_ENCODED_RUSTFLAGS", None)
                    env["RUSTFLAGS"] = "--cfg tokio_unstable"
                    command(["cargo", f"+{channel}", "test", "--quiet", "--release", "--features", "runtime-diagnostics", "--manifest-path", root / "Cargo.toml", "--target-dir", root / "target-runtime", "--locked", "--", "--test-threads=1"], env=env, log=root / "runtime-test.log")
            if kind == "server":
                for mode, feature in (("timing", "server-timing"), ("dial9", "dial9")):
                    if mode in modes:
                        env = cargo_env()
                        env.pop("CARGO_ENCODED_RUSTFLAGS", None)
                        env["RUSTFLAGS"] = RUSTFLAGS.get(mode, "")
                        command(["cargo", f"+{channel}", "test", "--quiet", "--release", "--features", feature, "--manifest-path", root / "Cargo.toml", "--target-dir", root / f"target-{mode}", "--locked", "--", "--test-threads=1"], env=env, log=root / f"{mode}-test.log")
    validate_generation(cache)
    if sources_before != tool_hash():
        raise SystemExit("Benchmark sources changed during compilation. Run build.py again.")
    write_json(cache / "build.json", {
        "tool_hash": sources_before, "compiler": compiler, "toolchain": channel, "binaries": binaries, "locks": locks,
        "modes": list(modes), "rustflags": {m: RUSTFLAGS.get(m, "") for m in modes},
        "generation_hash": digest(cache / "generation.json"),
    })


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--cache", type=Path, default=HERE / "artifacts/cache")
    p.add_argument("--modes", nargs="+", choices=MODES, default=MODES, help="Build selected modes; cpu is also needed for the shared client")
    a = p.parse_args()
    if "cpu" not in a.modes:
        p.error("--modes must include cpu for the shared client and preflight")
    build(a.cache.resolve(), a.modes)
