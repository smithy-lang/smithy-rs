#!/usr/bin/env python3
"""Fresh C10 boundary audit. Exit 1 records differences, 2 an incomplete run."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

from run import ROOT, SUITE, runtime_snapshot, source_files, fingerprints, compare_protocol

TEST = "software.amazon.smithy.rust.codegen.server.smithy.protocols.DocumentDepthCompatibilityTest"
KOTLIN_TEST = ROOT / "codegen-server/src/test/kotlin/software/amazon/smithy/rust/codegen/server/smithy/protocols/DocumentDepthCompatibilityTest.kt"

PROTOCOLS = ["rest-json", "aws-json-10", "aws-json-11"]
DEPTHS = [0, 1, 124, 125, 126, 127, 128, 129, 253, 254, 255, 256, 257]


def corpus(protocol):
    cases = []
    for binding in (["field", "payload"] if protocol == "rest-json" else ["field"]):
        for shape in ["array", "object", "mixed", "empty-array", "empty-object"]:
            for depth in DEPTHS:
                # Depth is the number of Document containers, excluding the
                # operation's input structure. Empty leaves are containers too.
                if shape.startswith("empty-") and depth == 0:
                    continue
                value = "[]" if shape == "empty-array" else "{}" if shape == "empty-object" else "0"
                for level in range(depth - int(shape.startswith("empty-"))):
                    if shape in ["array", "empty-array"] or (shape == "mixed" and level % 2 == 0):
                        value = "[" + value + "]"
                    else:
                        value = '{"k":' + value + "}"
                body = '{"document":' + value + "}" if binding == "field" else value
                case = {"id": f"{binding}-{shape}-{depth}", "method": "POST",
                        "uri": "/body" if binding == "field" else "/document-payload",
                        "body": body, "headers": [], "binding": binding,
                        "shape": shape, "depth": depth,
                        "source": "C10: expect_document_inner versus JsonDeserializer::read_document"}
                if protocol != "rest-json":
                    case.update(uri="/", content_type="application/x-amz-json-1." + ("0" if protocol == "aws-json-10" else "1"),
                                headers=[["x-amz-target", ("Json10Depth" if protocol == "aws-json-10" else "Json11Depth") + ".Body"]])
                cases.append(case)
    return cases


def compare(run):
    reports = [compare_protocol(run, protocol) for protocol in PROTOCOLS]
    (run / "differences.json").write_text(json.dumps(reports, indent=2) + "\n")
    summary = []
    for protocol in PROTOCOLS:
        cases = json.loads((run / f"cases/{protocol}.json").read_text())
        observations = {}
        for mode in ["legacy", "schema"]:
            observations[mode] = {row["id"]: row for row in map(json.loads, (run / f"{protocol}-{mode}.jsonl").read_text().splitlines())}
        for case in cases:
            rows = {mode: observations[mode][case["id"]] for mode in observations}
            for row in rows.values():
                if row["panic"] or row["status"] not in [200, 400]:
                    raise RuntimeError(f"Unexpected outcome: {protocol}/{case['id']}")
                if (row["status"] == 200) != (row["input"] is not None):
                    raise RuntimeError(f"Handler control failed: {protocol}/{case['id']}")
            summary.append({"protocol": protocol, **{k: case[k] for k in ["id", "binding", "shape", "depth"]},
                            "status": {mode: row["status"] for mode, row in rows.items()},
                            "equal": {key: rows["legacy"][key] == rows["schema"][key]
                                      for key in ["status", "headers", "wire_hex", "input"]}})
    (run / "boundary-summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(f"Evidence: {run}")
    return int(any(report["semantic_or_wire_differences"] for report in reports))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compare", type=Path)
    args = parser.parse_args()
    if args.compare:
        return compare(args.compare.resolve())
    run = SUITE / ".runs" / ("C10-" + datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ"))
    run.mkdir(parents=True)
    print(f"C10 run: {run}", flush=True)
    env = dict(os.environ, SCHEMA_COMPAT_ROOT=str(ROOT), SCHEMA_COMPAT_RUN=str(run),
               SCHEMA_COMPAT_PROTOCOLS=",".join(PROTOCOLS), SCHEMA_COMPAT_C10="1")
    env["SCHEMA_COMPAT_RUNTIME"] = str(runtime_snapshot(run))
    env.setdefault("CARGO_TARGET_DIR", str(SUITE / ".runs/target"))
    (run / "cases").mkdir()
    for protocol in PROTOCOLS:
        (run / f"cases/{protocol}.json").write_text(json.dumps(corpus(protocol), indent=2) + "\n")
    sources = [KOTLIN_TEST, SUITE / "run.py", Path(__file__).resolve()]
    sources.extend(SUITE.glob("*.rs.in"))
    sources.extend(SUITE.glob("models/*.smithy"))
    for directory in ["codegen-core/src/main", "codegen-server/src/main", "codegen-traits/src/main", "rust-runtime"]:
        sources.extend(source_files(ROOT / directory))
    before = fingerprints(sources)
    (run / "source-fingerprints.json").write_text(json.dumps(before, indent=2) + "\n")
    (run / "inputs").mkdir()
    for source in [KOTLIN_TEST, SUITE / "run.py", Path(__file__).resolve(), *SUITE.glob("*.rs.in"), *SUITE.glob("models/*.smithy")]:
        shutil.copyfile(source, run / "inputs" / source.name)
    # Select only runtime observations; unrelated compilation probes are outside C10.
    command = [str(ROOT / "gradlew"), ":codegen-server:test", "--tests", TEST,
               "--rerun-tasks", "--console=plain", "--info"]
    metadata = {"protocols": PROTOCOLS, "command": command, "python": sys.version,
                "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
                "git_head": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
                "settings": {mode: {"codegen": {"http-1x": True, "schemaSerde": mode == "schema"},
                                     "runtimeConfig": {"relativePath": env["SCHEMA_COMPAT_RUNTIME"]}}
                             for mode in ["legacy", "schema"]},
                "cargo_target_dir": env["CARGO_TARGET_DIR"],
                "tracked_diff_sha256": hashlib.sha256(subprocess.check_output(["git", "diff", "HEAD"])).hexdigest()}
    (run / "provenance.json").write_text(json.dumps(metadata, indent=2) + "\n")
    with (run / "gradle.log").open("w") as log:
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
    if result.returncode:
        print(f"Generation/build failed: {run / 'gradle.log'}", file=sys.stderr)
        return 2
    changed = [name for name, digest in fingerprints(sources).items() if digest != before[name]]
    (run / "source-stability.json").write_text(json.dumps({"changed_during_run": changed}, indent=2) + "\n")
    if changed:
        raise RuntimeError(f"Sources changed during audit: {changed}")
    return compare(run)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, RuntimeError, ValueError, KeyError) as error:
        print(f"Incomplete C10 audit: {error}", file=sys.stderr)
        sys.exit(2)
