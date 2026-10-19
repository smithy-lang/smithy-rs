#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Regress C04 against newly generated legacy REST JSON and REST XML servers."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

SUITE = Path(__file__).resolve().parent
ROOT = SUITE.parents[1]
KOTLIN_TEST = ROOT / ("codegen-server/src/test/kotlin/software/amazon/smithy/rust/"
                      "codegen/server/smithy/protocols/ResponsePrefixHeaderMediaTypeCompatibilityTest.kt")


def source_files(directory):
    for parent, dirs, names in os.walk(directory):
        dirs[:] = sorted(d for d in dirs if not d.startswith(".") and d not in {"target", "build", "__pycache__", "fuzz"})
        for name in sorted(names):
            if not name.lower().endswith((".md", ".markdown")):
                yield Path(parent) / name


def fingerprints(paths):
    return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}


def runtime_snapshot(run):
    """Copy current runtime source, omitting missing documentation includes only."""
    source = ROOT / "rust-runtime"
    target = run / "runtime"
    def ignore(directory, names):
        return [n for n in names if n in {"target", ".git", "fuzz", "__pycache__"}
                or n.lower().endswith((".md", ".markdown"))]
    target.mkdir()
    for crate in source.iterdir():
        if crate.is_dir() and (crate / "Cargo.toml").is_file():
            shutil.copytree(crate, target / crate.name, ignore=ignore)
    omitted = []
    pattern = re.compile(r'#\!?\[doc\s*=\s*include_str!\("([^"\n]+\.(?:md|markdown))"\)\]')
    for path in target.rglob("*.rs"):
        text = path.read_text()
        def replace(match):
            omitted.append({"file": str(path.relative_to(target)),
                            "line": text[:match.start()].count("\n") + 1,
                            "attribute": match.group(0)})
            prefix = "#!" if match.group(0).startswith("#!") else "#"
            return prefix + '[doc = "Documentation omitted in the isolated compatibility build."]'
        changed = pattern.sub(replace, text)
        if changed != text:
            path.write_text(changed)
    (run / "documentation-omissions.json").write_text(json.dumps(omitted, indent=2) + "\n")
    return target


def main():
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    run = SUITE / ".runs" / f"C04-{stamp}"
    run.mkdir(parents=True)
    env = dict(os.environ, SCHEMA_COMPAT_ROOT=str(ROOT), SCHEMA_COMPAT_RUN=str(run))
    env["SCHEMA_COMPAT_RUNTIME"] = str(runtime_snapshot(run))
    env.setdefault("CARGO_TARGET_DIR", str(SUITE / ".runs/target"))
    test = KOTLIN_TEST
    sources = [SUITE / "run_c04.py", test, SUITE / "models/response-prefix-header-media-type.smithy"]
    for directory in ["codegen-core/src/main", "codegen-server/src/main", "codegen-traits/src/main", "rust-runtime"]:
        sources.extend(source_files(ROOT / directory))
    (run / "source-fingerprints.json").write_text(json.dumps(fingerprints(sources), indent=2) + "\n")
    command = [str(ROOT / "gradlew"), ":codegen-server:test", "--tests",
               "software.amazon.smithy.rust.codegen.server.smithy.protocols.ResponsePrefixHeaderMediaTypeCompatibilityTest",
               "--rerun-tasks", "--console=plain"]
    print(f"C04 run: {run}", flush=True)
    with (run / "gradle.log").open("w") as log:
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
    reports = []
    for service in ["JsonResponsePrefixMedia", "XmlResponsePrefixMedia"]:
        paths = [run / f"{service}-{mode}.json" for mode in ["legacy", "schema"]]
        if all(path.is_file() for path in paths):
            legacy, schema = [json.loads(path.read_text()) for path in paths]
            if not legacy or len(legacy) != len(schema):
                raise RuntimeError(f"Missing observations: {service}")
            differences = [{"legacy": left, "schema": right}
                           for left, right in zip(legacy, schema) if left != right]
            reports.append({"service": service, "legacy_requests": len(legacy),
                            "schema_requests": len(schema), "differences": differences})
            print(f"{service}: {len(legacy)} legacy / {len(schema)} schema requests; {len(differences)} differences")
    if len(reports) != 2 or any(report["differences"] for report in reports):
        result.returncode = result.returncode or 1
    (run / "comparison.json").write_text(json.dumps(reports, indent=2) + "\n")
    print(f"{'PASS' if result.returncode == 0 else 'FAIL'}: {run / 'gradle.log'}")
    return result.returncode


if __name__ == "__main__":
    sys.exit(main())
