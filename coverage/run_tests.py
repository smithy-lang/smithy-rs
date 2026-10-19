#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Execute every configured projection, retaining failures instead of excluding them."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time
from datetime import datetime, timezone
import uuid


def fingerprint(paths):
    digest = hashlib.sha256()
    for path in sorted(paths):
        files = sorted(path.rglob("*")) if path.is_dir() else [path]
        for item in files:
            if item.is_file():
                digest.update(str(item).encode())
                digest.update(hashlib.sha256(item.read_bytes()).digest())
    return digest.hexdigest()


def execute(command, log, env, timeout, cwd=None):
    started = time.monotonic()
    with log.open("w") as output:
        try:
            result = subprocess.run(
                command, cwd=cwd, env=env, stdout=output,
                stderr=subprocess.STDOUT, timeout=timeout, check=False,
            )
            code = result.returncode
        except (OSError, subprocess.TimeoutExpired) as error:
            output.write(f"\nRUNNER ERROR: {error}\n")
            code = -1
    return {"command": command, "exit_code": code,
            "seconds": round(time.monotonic() - started, 2), "log": str(log)}


def audit_generated(crate):
    """Do not allow expected-panic or ignored tests to turn a failure into a pass."""
    ids, suppressed, unsupported = [], [], []
    for source in sorted(crate.rglob("*.rs")):
        if "target" in source.relative_to(crate).parts:
            continue
        text = source.read_text()
        ids.extend(re.findall(r"Test ID: ([^\r\n]+)", text))
        for number, line in enumerate(text.splitlines(), 1):
            if re.search(r"#\s*\[\s*(?:should_panic|ignore)(?:\W|$)", line):
                suppressed.append(f"{source}:{number}: {line.strip()}")
            if line.lstrip().startswith(("//", "/*")) and re.search(
                r"(?i)\b(?:test(?:s|ing)?|serializ\w*|deserializ\w*)\b\s+.*"
                r"\b(?:not (?:yet )?supported|unsupported|not implemented)\b", line,
            ):
                unsupported.append(f"{source}:{number}: {line.strip()}")
    return {"protocol_test_ids": sorted(ids), "suppressed_assertions": suppressed,
            "unsupported_test_comments": unsupported}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path)
    parser.add_argument("--classpath", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--timeout", type=int, default=1200,
                        help="Maximum seconds per generation or test stage; timeout is a failure")
    parser.add_argument("--projection", action="append",
                        help="Explicit diagnostic subset; report remains labeled partial")
    parser.add_argument("--offline", action="store_true",
                        help="Use only cached Cargo dependencies; missing dependencies fail")
    args = parser.parse_args()
    coverage = Path(__file__).resolve().parent
    root = coverage.parent
    config_path = args.config or coverage / "build/smithy-build.json"
    classpath_path = args.classpath or coverage / "build/codegen-classpath.txt"
    run_name = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + uuid.uuid4().hex[:8]
    output = (args.output or coverage / "build/strict-tests" / run_name).resolve()
    if not config_path.is_file() or not classpath_path.is_file():
        parser.error("Run ./gradlew :coverage:prepareStrictCorpus first")
    config = json.loads(config_path.read_text())
    classpath = classpath_path.read_text().strip()
    classpath_entries = [Path(entry) for entry in classpath.split(os.pathsep)]
    runtime_sources = [
        path
        for directory in [root / "rust-runtime", root / "aws/rust-runtime"]
        for source_kind in ["src", "tests", "benches"]
        for path in directory.glob(f"*/{source_kind}/**/*.rs")
    ]
    initial_classpath_fingerprint = fingerprint(classpath_entries)
    initial_runtime_fingerprint = fingerprint(runtime_sources)
    output.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, SMITHY_RS_STRICT_PROTOCOL_TESTS="1")
    env.setdefault("CARGO_BUILD_JOBS", "4")
    env.setdefault("CARGO_TARGET_DIR", str(output / "cargo-target"))
    selected = args.projection or list(config["projections"])
    unknown = set(selected) - set(config["projections"])
    if unknown:
        parser.error(f"Unknown projections: {sorted(unknown)}")
    results = []
    report = {
        "scope": "partial" if args.projection else "all configured projections",
        "configuration": str(config_path.resolve()),
        "configuration_sha256": hashlib.sha256(config_path.read_bytes()).hexdigest(),
        "codegen_classpath": classpath.split(os.pathsep),
        "codegen_classpath_sha256": initial_classpath_fingerprint,
        "runtime_source_sha256": initial_runtime_fingerprint,
        "strict_protocol_assertions": True,
        "baseline": "current-tree schemaSerde=false; historical baseline requires a separate clean checkout",
        "comparison": "independent specification tests, supplemented by five protocol A/B server fixtures",
        "expected_projections": list(config["projections"]),
        "results": results,
    }

    def save():
        (output / "results.json").write_text(json.dumps(report, indent=2) + "\n")

    for name in selected:
        projection = config["projections"][name]
        case_dir = output / name
        case_dir.mkdir(parents=True, exist_ok=True)
        one_config = case_dir / "smithy-build.json"
        one_config.write_text(json.dumps({**config, "projections": {name: projection}}, indent=2) + "\n")
        case = {"projection": name, "plugins": projection["plugins"],
                "imports": [{"path": path,
                             "sha256": hashlib.sha256(Path(path).read_bytes()).hexdigest()}
                            for path in projection.get("imports", [])]}
        case["generation"] = execute(
            ["java", "-Xmx4g", "-cp", classpath, "software.amazon.smithy.cli.SmithyCli",
             "build", "--config", str(one_config), "--output", str(case_dir / "generated"), "--discover"],
            case_dir / "generation.log", env, args.timeout, root,
        )
        case["artifacts"] = []
        case["status"] = "running"
        results.append(case)
        save()
        if case["generation"]["exit_code"]:
            case["status"] = "generation failed"
            print(f"FAIL {name}: generation", flush=True)
            save()
            continue
        for plugin in projection["plugins"]:
            crate = case_dir / "generated" / name / plugin
            artifact = {"plugin": plugin, "crate": str(crate), "status": "running"}
            case["artifacts"].append(artifact)
            save()
            if not (crate / "Cargo.toml").is_file():
                artifact["status"] = "missing generated crate"
                continue
            artifact.update(audit_generated(crate))
            command = ["cargo", "test", "--quiet", "--lib", "--tests",
                       "--manifest-path", str(crate / "Cargo.toml")]
            if args.offline:
                command.append("--offline")
            artifact["execution"] = execute(command, case_dir / f"{plugin}-tests.log",
                                            env, args.timeout, crate)
            text = Path(artifact["execution"]["log"]).read_text()
            outcomes = re.findall(r"test result: (\w+)\.\s+(\d+) passed;\s+(\d+) failed;\s+(\d+) ignored", text)
            artifact["passed"] = sum(int(row[1]) for row in outcomes)
            artifact["failed"] = sum(int(row[2]) for row in outcomes)
            artifact["ignored"] = sum(int(row[3]) for row in outcomes)
            if artifact["execution"]["exit_code"]:
                artifact["status"] = "tests or compilation failed"
            elif artifact["suppressed_assertions"] or artifact["ignored"]:
                artifact["status"] = "suppressed assertions"
            elif not artifact["protocol_test_ids"]:
                artifact["status"] = "no executable protocol assertions"
            elif not artifact["passed"]:
                artifact["status"] = "no executed tests"
            elif artifact["unsupported_test_comments"]:
                artifact["status"] = "unsupported protocol assertions"
            else:
                artifact["status"] = "passed"
        case["status"] = ("passed" if case["artifacts"] and
                          all(a["status"] == "passed" for a in case["artifacts"]) else "failed or unverified")
        print(f"{'PASS' if case['status'] == 'passed' else 'FAIL'} {name}: " +
              ", ".join(f"{a['plugin']}: {a['status']}" for a in case["artifacts"]), flush=True)
        save()
    report["passed_projections"] = sum(case["status"] == "passed" for case in results)
    report["failed_or_unverified_projections"] = len(results) - report["passed_projections"]
    report["complete"] = set(selected) == set(config["projections"]) and len(results) == len(selected)
    report["inputs_unchanged"] = (
        fingerprint(classpath_entries) == initial_classpath_fingerprint and
        fingerprint(runtime_sources) == initial_runtime_fingerprint
    )
    save()
    print(f"Report: {output / 'results.json'}")
    return int(bool(report["failed_or_unverified_projections"]) or not report["complete"] or
               not report["inputs_unchanged"])


if __name__ == "__main__":
    sys.exit(main())
