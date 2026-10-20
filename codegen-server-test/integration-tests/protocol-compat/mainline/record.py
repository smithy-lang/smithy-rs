#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Record shared scenarios using a clean, pinned legacy codegen/runtime tree."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile


SUITE = Path(__file__).resolve().parents[1]
ROOT = SUITE.parents[2]
DEFAULT_REF = "06c62da902fba6059ab34d0dc3d570907560cd3a"
SERVICES = {
    "rest": "ReviewRest",
    "awsjson": "ReviewAwsJson",
    "cbor": "ReviewCbor",
    "document": "ReviewDocument",
}


def command(args, cwd, log):
    print("Running: " + " ".join(map(str, args)), flush=True)
    with log.open("a") as output:
        subprocess.run(args, cwd=cwd, stdout=output, stderr=subprocess.STDOUT, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mainline-ref", default=DEFAULT_REF, help="Local Git commit used for both codegen and runtime")
    parser.add_argument("--output", type=Path, default=SUITE / "baselines/mainline.json")
    args = parser.parse_args()
    commit = subprocess.check_output(
        ["git", "rev-parse", "--verify", f"{args.mainline_ref}^{{commit}}"],
        cwd=ROOT,
        text=True,
    ).strip()
    run = Path(tempfile.mkdtemp(prefix="smithy-mainline-baseline-"))
    tree = run / "source"
    tree.mkdir()
    log = run / "record.log"
    print(f"Legacy source: {commit}\nEvidence directory: {run}", flush=True)

    # Export committed source only. Pending changes in either checkout cannot enter
    # the oracle; the caller's checkout and Git refs are left untouched.
    archive = run / "source.tar"
    with archive.open("wb") as output:
        subprocess.run(["git", "archive", commit], cwd=ROOT, stdout=output, check=True)
    with tarfile.open(archive) as source:
        source.extractall(tree, filter="data")
    archive.unlink()

    models = tree / "codegen-server-test/custom-test-models/mainline-baseline"
    models.mkdir()
    inputs = [
        SUITE / "models/protocol-compat-services.smithy",
        SUITE / "models/protocol-compat-operations.smithy",
        SUITE / "src/handlers.rs",
        SUITE / "tests/support/mod.rs",
        SUITE / "tests/support/scenarios.rs",
    ]
    for source in inputs[:2]:
        shutil.copyfile(source, models / source.name)
    projections = []
    for protocol, service in SERVICES.items():
        projections.append(
            f'''    CodegenTest(
        "com.example.review#{service}",
        "compat-{protocol}-mainline-sdk",
        imports = listOf("custom-test-models/mainline-baseline/protocol-compat-services.smithy",
                         "custom-test-models/mainline-baseline/protocol-compat-operations.smithy"),
        extraCodegenConfig = """"http-1x": true, "schemaSerde": false""",
    )'''
        )
    build = tree / "codegen-server-test/build.gradle.kts"
    text, matches = re.subn(
        r"^val allCodegenTests = .+$",
        lambda _: "val allCodegenTests = listOf(\n" + ",\n".join(projections) + "\n)",
        build.read_text(),
        count=1,
        flags=re.MULTILINE,
    )
    if matches != 1:
        raise RuntimeError("Cannot locate the legacy allCodegenTests declaration")
    build.write_text(text)
    command(
        [str(tree / "gradlew"), ":codegen-server-test:smithyBuild", "--console", "plain"],
        tree,
        log,
    )
    recorder = tree / "codegen-server-test/integration-tests/protocol-compat-mainline"
    (recorder / "src/support").mkdir(parents=True)
    for filename in ("lib.rs", "main.rs"):
        shutil.copyfile(SUITE / "mainline" / (filename + ".in"), recorder / "src" / filename)
    shutil.copyfile(inputs[2], recorder / "src/handlers.rs")
    for source in inputs[3:]:
        shutil.copyfile(source, recorder / "src/support" / source.name)
    dependencies = [
        'bytes = "1"',
        'http = "1"',
        'http-body = "1"',
        'http-body-util = "0.1"',
        'tower = { version = "0.4", features = ["util"] }',
        'tokio = { version = "1", features = ["macros", "rt-multi-thread"] }',
        'minicbor = { version = "2.2.1", features = ["alloc", "half"] }',
        'serde_json = "1"',
        'aws-smithy-http-server = { path = "../../../rust-runtime/aws-smithy-http-server" }',
        'aws-smithy-types = { path = "../../../rust-runtime/aws-smithy-types" }',
    ]
    for protocol in SERVICES:
        module = f"compat-{protocol}-mainline-sdk"
        dependencies.append(
            f'compat-{protocol}-server-sdk = {{ package = "{module}", '
            f'path = "../../build/smithyprojections/codegen-server-test/{module}/rust-server-codegen" }}'
        )
    (recorder / "Cargo.toml").write_text(
        '[package]\nname = "protocol-compat-mainline"\nversion = "0.0.0"\n'
        'edition = "2021"\npublish = false\n\n[workspace]\n\n[dependencies]\n'
        + "\n".join(dependencies) + "\n"
    )
    snapshots = run / "mainline.json"
    command(
        ["cargo", "run", "--quiet", "--manifest-path", str(recorder / "Cargo.toml"), "--", str(snapshots)],
        tree,
        log,
    )
    records = json.loads(snapshots.read_text())
    if not records or not all(isinstance(value, str) for value in records.values()):
        raise RuntimeError("The legacy recorder produced no usable snapshots")
    metadata = {
        "commit": commit,
        "schemaSerde": False,
        "http-1x": True,
        "scenario_count": len(records),
        "inputs": {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest() for path in inputs},
    }
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(snapshots, output)
    output.with_suffix(".metadata.json").write_text(json.dumps(metadata, indent=2, sort_keys=True) + "\n")
    print(f"Recorded {len(records)} legacy responses in {output}\nBuild log: {log}", flush=True)


if __name__ == "__main__":
    main()
