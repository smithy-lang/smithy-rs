#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Regress C03 against fresh REST JSON and REST XML servers with generated serde."""
import datetime
import json
import os
import sys

import run_c05 as audit


def main():
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    run = audit.SUITE / ".runs" / f"C03-{stamp}"
    run.mkdir(parents=True)
    env = dict(os.environ, SCHEMA_COMPAT_ROOT=str(audit.ROOT), SCHEMA_COMPAT_RUN=str(run))
    env["SCHEMA_COMPAT_RUNTIME"] = str(audit.runtime_snapshot(run))
    env.setdefault("CARGO_TARGET_DIR", str(audit.SUITE / ".runs/target"))
    test = audit.KOTLIN_TEST.with_name("PrefixHeaderMediaTypeCompatibilityTest.kt")
    sources = [audit.SUITE / "run_c03.py", audit.SUITE / "run_c05.py", test, audit.SUITE / "models/prefix-header-media-type.smithy"]
    for directory in ["codegen-core/src/main", "codegen-server/src/main", "codegen-traits/src/main", "rust-runtime"]:
        sources.extend(audit.source_files(audit.ROOT / directory))
    (run / "source-fingerprints.json").write_text(json.dumps(audit.fingerprints(sources), indent=2) + "\n")
    command = [str(audit.ROOT / "gradlew"), ":codegen-server:test", "--tests",
               "software.amazon.smithy.rust.codegen.server.smithy.protocols.PrefixHeaderMediaTypeCompatibilityTest",
               "--rerun-tasks", "--console=plain", "--info"]
    print(f"C03 run: {run}", flush=True)
    with (run / "gradle.log").open("w") as log:
        result = audit.run_logged(command, cwd=audit.ROOT, env=env, log=log)
    reports = []
    failed = result.returncode != 0
    for service in ["JsonPrefixMedia", "XmlPrefixMedia"]:
        paths = [run / f"{service}-{mode}.json" for mode in ["legacy", "schema"]]
        if all(path.is_file() for path in paths):
            legacy, schema = [json.loads(path.read_text()) for path in paths]
            expected_panics = {"empty", "two-empty", "empty-then-invalid-wire", "empty-then-invalid-base64"}
            assert len(legacy) == len(schema) == 36
            assert {row["name"] for row in legacy if row["panic"]} == expected_panics
            rejection = next(row for row in legacy if row["name"] == "repeated")
            differences = []
            exceptions = []
            for left, right in zip(legacy, schema):
                expected = dict(rejection, name=left["name"]) if left["panic"] else left
                if right != expected:
                    differences.append({"legacy": left, "schema": right, "expected": expected})
                elif left["panic"]:
                    exceptions.append({"legacy": left, "schema": right})
            failed |= bool(differences)
            reports.append({"service": service, "legacy_requests": len(legacy),
                            "schema_requests": len(schema), "differences": differences,
                            "intentional_panic_to_error_changes": exceptions})
            print(f"{service}: {len(legacy)} legacy / {len(schema)} schema requests; {len(differences)} unexpected differences; {len(exceptions)} intentional panic-to-error changes")
        else:
            failed = True
    (run / "comparison.json").write_text(json.dumps(reports, indent=2) + "\n")
    print(f"{'PASS' if not failed else 'FAIL'}: {run / 'gradle.log'}")
    return int(failed)


if __name__ == "__main__":
    sys.exit(main())
