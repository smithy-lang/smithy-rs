#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Compare duplicate JSON null members using fresh generated servers."""
import datetime
import json
import os
import sys
from run_c05 import ROOT, SUITE, runtime_snapshot, source_files, fingerprints, run_logged
KOTLIN_TEST = ROOT / "codegen-server/src/test/kotlin/software/amazon/smithy/rust/codegen/server/smithy/protocols/json/DuplicateJsonNullCompatibilityTest.kt"


def main():
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    run = SUITE / ".runs" / f"C06-{stamp}"
    run.mkdir(parents=True)
    env = dict(os.environ, SCHEMA_COMPAT_ROOT=str(ROOT), SCHEMA_COMPAT_RUN=str(run))
    env["SCHEMA_COMPAT_RUNTIME"] = str(runtime_snapshot(run))
    env.setdefault("CARGO_TARGET_DIR", str(SUITE / ".runs/target"))
    test = KOTLIN_TEST
    sources = [SUITE / "run_c06.py", test, SUITE / "models/duplicate-json-null.smithy"]
    for directory in ["codegen-core/src/main", "codegen-server/src/main", "codegen-traits/src/main", "rust-runtime"]:
        sources.extend(source_files(ROOT / directory))
    (run / "source-fingerprints.json").write_text(json.dumps(fingerprints(sources), indent=2) + "\n")
    command = [str(ROOT / "gradlew"), ":codegen-server:test", "--tests",
               "software.amazon.smithy.rust.codegen.server.smithy.protocols.json.DuplicateJsonNullCompatibilityTest",
               "--rerun-tasks", "--console=plain", "--info"]
    print(f"C06 run: {run}", flush=True)
    with (run / "gradle.log").open("w") as log:
        result = run_logged(command, cwd=ROOT, env=env, log=log)
    reports = []
    for service in ["RestDuplicateNull", "Json10DuplicateNull", "Json11DuplicateNull"]:
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
    if len(reports) != 3 or any(report["differences"] for report in reports):
        result.returncode = result.returncode or 1
    (run / "comparison.json").write_text(json.dumps(reports, indent=2) + "\n")
    print(f"{'PASS' if result.returncode == 0 else 'FAIL'}: {run / 'gradle.log'}")
    return result.returncode


if __name__ == "__main__":
    sys.exit(main())
