#!/usr/bin/env python3
"""Regenerate and test corrected legacy/schema response bindings from current source."""
import datetime
import json
import os
from pathlib import Path
import subprocess
import sys

import run as audit


def main():
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    run = audit.SUITE / ".runs" / f"response-bindings-{stamp}"
    run.mkdir(parents=True)
    env = dict(os.environ, SCHEMA_COMPAT_ROOT=str(audit.ROOT), SCHEMA_COMPAT_RUN=str(run))
    env["SCHEMA_COMPAT_RUNTIME"] = str(audit.runtime_snapshot(run))
    env.setdefault("CARGO_TARGET_DIR", str(audit.SUITE / ".runs/target"))
    tests = ["ServerResponseBindingsTest", "AwsJson10StrictCompatibilityTest", "AwsJson11StrictCompatibilityTest"]
    sources = list((audit.SUITE / "models").glob("response-*.smithy")) + [__file__]
    sources = [Path(path) for path in sources]
    for directory in ["codegen-core/src/main", "codegen-server/src/main", "codegen-traits/src/main", "rust-runtime"]:
        sources.extend(audit.source_files(audit.ROOT / directory))
    sources.extend(audit.KOTLIN_TEST.with_name(test + ".kt") for test in tests)
    (run / "source-fingerprints.json").write_text(json.dumps(audit.fingerprints(sources), indent=2) + "\n")
    command = [str(audit.ROOT / "gradlew"), ":codegen-server:test"]
    for test in tests:
        command.extend(["--tests", "software.amazon.smithy.rust.codegen.server.smithy.protocols." + test])
    command.extend(["--rerun-tasks", "--console=plain"])
    print(f"Response-binding run: {run}", flush=True)
    with (run / "gradle.log").open("w") as log:
        result = subprocess.run(command, cwd=audit.ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
    comparisons = {}
    if result.returncode == 0:
        for protocol, suffix in [("aws-json-10", "txt"), ("aws-json-11", "tsv")]:
            directory = run / f"{protocol}-strict"
            legacy = (directory / f"legacy.{suffix}").read_text()
            schema = (directory / f"schema.{suffix}").read_text()
            if legacy != schema:
                raise RuntimeError(f"Response mismatch in {directory}")
            count = len(legacy.splitlines()) - (1 if suffix == "tsv" else 0)
            if count <= 0:
                raise RuntimeError(f"Missing observations in {directory}")
            comparisons[protocol] = count
            print(f"{protocol}: {count} matching HTTP responses", flush=True)
    (run / "result.json").write_text(json.dumps({"tests": tests, "exit_code": result.returncode,
                                                "matching_responses": comparisons}, indent=2) + "\n")
    print(f"{'PASS' if result.returncode == 0 else 'FAIL'}: {run / 'gradle.log'}", flush=True)
    return result.returncode


if __name__ == "__main__":
    sys.exit(main())
