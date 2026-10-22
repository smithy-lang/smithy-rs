#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Regress C01 against fresh REST JSON and REST XML servers with generated serde."""
import datetime
import json
import os
import subprocess
import sys

import run as audit


def main():
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    run = audit.SUITE / ".runs" / f"C01-{stamp}"
    run.mkdir(parents=True)
    env = dict(os.environ, SCHEMA_COMPAT_ROOT=str(audit.ROOT), SCHEMA_COMPAT_RUN=str(run))
    env["SCHEMA_COMPAT_RUNTIME"] = str(audit.runtime_snapshot(run))
    env.setdefault("CARGO_TARGET_DIR", str(audit.SUITE / ".runs/target"))
    test = audit.KOTLIN_TEST.with_name("TrailingSlashLabelsCompatibilityTest.kt")
    sources = [test, audit.SUITE / "models/trailing-slash-labels.smithy"]
    for directory in ["codegen-core/src/main", "codegen-server/src/main", "codegen-traits/src/main", "rust-runtime"]:
        sources.extend(audit.source_files(audit.ROOT / directory))
    (run / "source-fingerprints.json").write_text(json.dumps(audit.fingerprints(sources), indent=2) + "\n")
    command = [str(audit.ROOT / "gradlew"), ":codegen-server:test", "--tests",
               "software.amazon.smithy.rust.codegen.server.smithy.protocols.routing.TrailingSlashLabelsCompatibilityTest",
               "--rerun-tasks", "--console=plain"]
    print(f"C01 run: {run}", flush=True)
    with (run / "gradle.log").open("w") as log:
        result = subprocess.run(command, cwd=audit.ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
    reports = []
    for service in ["JsonLabels", "XmlLabels"]:
        paths = [run / f"{service}-{mode}.json" for mode in ["legacy", "schema"]]
        if all(path.is_file() for path in paths):
            legacy, schema = [json.loads(path.read_text()) for path in paths]
            differences = [{"legacy": left, "schema": right}
                           for left, right in zip(legacy, schema) if left != right]
            reports.append({"service": service, "legacy_requests": len(legacy),
                            "schema_requests": len(schema), "differences": differences})
            print(f"{service}: {len(legacy)} legacy / {len(schema)} schema requests; {len(differences)} differences")
    (run / "comparison.json").write_text(json.dumps(reports, indent=2) + "\n")
    print(f"{'PASS' if result.returncode == 0 else 'FAIL'}: {run / 'gradle.log'}")
    return result.returncode


if __name__ == "__main__":
    sys.exit(main())
