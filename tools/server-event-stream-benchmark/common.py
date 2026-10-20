# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Shared provenance and subprocess helpers; no implicit generation or builds."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tomllib

HERE = Path(__file__).resolve().parent
# Pinned release for explicit historical comparisons.
DEFAULT_LEGACY_RELEASE = "release-2026-09-30.2"
PROTOCOLS = {"rest_json": "@aws.protocols#restJson1", "rpc_v2_cbor": "@smithy.protocols#rpcv2Cbor"}
EXCLUDED = {".git", ".gradle", "build", "target", "__pycache__", "artifacts", ".idea"}
SOURCE_ROOTS = ("codegen-core", "codegen-server", "codegen-client", "rust-runtime", "buildSrc", "gradle")


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def fingerprint(root, paths=None):
    root = Path(root)
    if paths is None:
        paths = []
        for name in SOURCE_ROOTS:
            base = root / name
            paths.extend(p for p in base.rglob("*") if p.is_file() and not set(p.relative_to(base).parts) & EXCLUDED)
        paths.extend(
            p for p in root.iterdir()
            if p.is_file() and (p.suffix in {".kts", ".toml", ".properties", ".gradle", ".json"} or p.name == "gradlew")
        )
    h = hashlib.sha256()
    for path in sorted(paths):
        h.update(str(path.relative_to(root)).encode())
        h.update(bytes.fromhex(digest(path)))
    return h.hexdigest()


def tool_hash():
    return fingerprint(HERE, [p for p in HERE.iterdir() if p.suffix in {".py", ".rs", ".gradle", ".smithy"}])


def generation_hash():
    return fingerprint(HERE, [HERE / n for n in ("prepare.py", "model.smithy", "codegen.init.gradle")])


def write_json(path, data):
    Path(path).write_text(json.dumps(data, indent=2) + "\n")


def command(args, cwd=None, env=None, log=None):
    print("Running:", " ".join(map(str, args)), flush=True)
    if log:
        with Path(log).open("w") as f:
            subprocess.run(list(map(str, args)), cwd=cwd, env=env, stdout=f, stderr=subprocess.STDOUT, check=True)
    else:
        subprocess.run(list(map(str, args)), cwd=cwd, env=env, check=True)


def cargo_env():
    return {**os.environ, "PATH": str(Path.home() / ".cargo/bin") + ":" + os.environ["PATH"]}


def toolchain(provenance):
    channels = []
    for side in ("legacy", "schema"):
        path = Path(provenance[side]["repo"]) / "rust-toolchain.toml"
        channels.append(tomllib.loads(path.read_text())["toolchain"]["channel"])
    if channels[0] != channels[1]:
        raise SystemExit(f"Checkouts pin different compilers: {channels}; choose an explicit compatible comparison")
    return channels[0]


def validate_generation(cache):
    data = json.loads((cache / "generation.json").read_text())
    if data["generation_hash"] != generation_hash():
        raise SystemExit("Generation inputs changed. Use a new cache and prepare again.")
    for side in ("legacy", "schema"):
        if fingerprint(data[side]["repo"]) != data[side]["source_hash"]:
            raise SystemExit(f"{side} sources changed since generation. Use a new cache.")
    for path, expected in data["generated_hashes"].items():
        if digest(cache / path) != expected:
            raise SystemExit(f"Generated source changed: {path}")
    for side in ("legacy", "schema"):
        for protocol in PROTOCOLS:
            src = cache / side / "generated" / f"{protocol}_server" / "rust-server-codegen" / "src"
            text = "\n".join(p.read_text() for p in src.rglob("*.rs"))
            markers = ("SchemaEventMarshaller", "SchemaEventUnmarshaller", "event_stream_response_body")
            if side == "schema" and not all(marker in text for marker in markers):
                raise SystemExit(f"{protocol}: generated current server does not use schema event streams")
            if side == "legacy" and any(marker in text for marker in markers):
                raise SystemExit(f"{protocol}: generated legacy server unexpectedly uses schema event streams")
    return data
