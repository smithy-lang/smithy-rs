#!/usr/bin/env python3
"""Generate both server paths from checked-in models; fail on observed differences."""
import argparse
import datetime
import hashlib
import json
import os
import re
import shutil
from pathlib import Path
import subprocess
import sys

SUITE = Path(__file__).resolve().parent
ROOT = SUITE.parents[1]
TEST = "software.amazon.smithy.rust.codegen.server.smithy.protocols.SourceDrivenSchemaSerdeCompatibilityTest"
PROTOCOLS = ["rest-json", "aws-json-10", "aws-json-11", "rpc-v2-cbor", "rest-xml"]
KOTLIN_TEST = ROOT / ("codegen-server/src/test/kotlin/software/amazon/smithy/rust/"
                      "codegen/server/smithy/protocols/SourceDrivenSchemaSerdeCompatibilityTest.kt")


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


def compare_protocol(run, protocol):
    rows = {}
    cases = json.loads((run / f"cases/{protocol}.json").read_text())
    expected = {case["id"] for case in cases}
    if len(expected) != len(cases):
        raise RuntimeError(f"Duplicate case IDs in {protocol}")
    for mode in ("legacy", "schema"):
        path = run / f"{protocol}-{mode}.jsonl"
        if not path.is_file():
            raise RuntimeError(f"Missing observations: {path}")
        entries = [json.loads(line) for line in path.read_text().splitlines()]
        rows[mode] = {row["id"]: row for row in entries}
        if set(rows[mode]) != expected or len(entries) != len(expected):
            raise RuntimeError(f"Incomplete/duplicate observations for {mode}")
    for case in cases:
        status = case.get("expected_legacy_status")
        if status is not None and rows["legacy"][case["id"]]["status"] != status:
            raise RuntimeError(f"Legacy control {protocol}/{case['id']} expected {status}; "
                               f"got {rows['legacy'][case['id']]['status']}. Check that the intended handler was reached.")
    differences = []
    for case_id, legacy in rows["legacy"].items():
        schema = rows["schema"][case_id]
        changed = {k: {"legacy": legacy[k], "schema": schema[k]}
                   for k in legacy if k != "id" and legacy[k] != schema[k]}
        if changed:
            differences.append({"id": case_id, "changed": changed})
    report = {"protocol": protocol, "cases": len(rows["legacy"]), "different_cases": len(differences),
              "semantic_or_wire_differences": sum(any(k != "logs" for k in d["changed"]) for d in differences),
              "wire_only_beyond_logs": sum(set(d["changed"]) - {"logs"} == {"wire_hex"} for d in differences),
              "panics": {mode: [r["id"] for r in rows[mode].values() if r["panic"]] for mode in rows},
              "sensitive_log_leaks": {mode: [r["id"] for r in rows[mode].values() if r.get("log_leaks")] for mode in rows},
              "differences": differences}
    print(f'{protocol}: {report["cases"]} cases; {report["different_cases"]} differ; '
          f'{report["semantic_or_wire_differences"]} differ beyond log text')
    for diff in differences:
        if set(diff["changed"]) != {"logs"}:
            print(f'  {diff["id"]}: {", ".join(diff["changed"])}')
    return report


def compare(run):
    protocols = json.loads((run / "provenance.json").read_text())["protocols"]
    reports = [compare_protocol(run, p) for p in protocols]
    probes = {}
    for line in (run / "compile-probes.tsv").read_text().splitlines():
        probe, mode, status = line.split("\t")
        probes.setdefault(probe, {})[mode] = int(status)
    if not probes or any(set(status) != {"legacy", "schema"} for status in probes.values()):
        raise RuntimeError("Missing compilation probe observations")
    for probe, status in probes.items():
        print(f"Compilation {probe}: {status}")
    (run / "differences.json").write_text(json.dumps({"protocols": reports, "compile_probes": probes}, indent=2) + "\n")
    print(f"Full evidence: {run / 'differences.json'}")
    return (any(r["different_cases"] or any(r["panics"].values()) or any(r["sensitive_log_leaks"].values()) for r in reports)
            or any(any(s.values()) for s in probes.values()))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compare", type=Path, help="Recompare a completed run without rebuilding")
    parser.add_argument("--protocols", nargs="+", choices=PROTOCOLS, default=PROTOCOLS)
    args = parser.parse_args()
    if args.compare:
        return int(compare(args.compare.resolve()))
    run = SUITE / ".runs" / datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    run.mkdir(parents=True)
    print(f"Run: {run}", flush=True)
    env = dict(os.environ, SCHEMA_COMPAT_ROOT=str(ROOT), SCHEMA_COMPAT_RUN=str(run))
    env["SCHEMA_COMPAT_PROTOCOLS"] = ",".join(args.protocols)
    env["SCHEMA_COMPAT_RUNTIME"] = str(runtime_snapshot(run))
    (run / "cases").mkdir()
    for protocol in args.protocols:
        shutil.copyfile(SUITE / f"cases/{protocol}.json", run / f"cases/{protocol}.json")
    env.setdefault("CARGO_TARGET_DIR", str(SUITE / ".runs/target"))
    metadata = {
        "protocols": args.protocols,
        "rustc": subprocess.check_output(["rustc", "--version"], cwd=ROOT, text=True).strip(),
        "python": sys.version,
        "corpus_rustflags": {"default": "-D warnings", "rest-xml": "-D warnings -A dead_code"},
        "compilation_probe_rustflags": "-D warnings",
        "git_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "tracked_diff_sha256": hashlib.sha256(subprocess.check_output(
            ["git", "diff", "HEAD", "--", ":!*.md", ":!*.markdown"], cwd=ROOT)).hexdigest(),
        "suite_sha256": fingerprints([*source_files(SUITE), KOTLIN_TEST]),
    }
    source_paths = []
    for relative in ["codegen-core/src/main", "codegen-server/src/main", "codegen-traits/src/main", "rust-runtime"]:
        source_paths.extend(source_files(ROOT / relative))
    (run / "source-fingerprints.json").write_text(json.dumps(fingerprints(source_paths), indent=2) + "\n")
    (run / "provenance.json").write_text(json.dumps(metadata, indent=2) + "\n")
    command = [str(ROOT / "gradlew"), ":codegen-server:test", "--tests", TEST,
               "--rerun-tasks", "--console=plain"]
    with (run / "gradle.log").open("w") as log:
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
    if result.returncode:
        print(f"Generation/build failed; inspect {run / 'gradle.log'}", file=sys.stderr)
        return 2
    return int(compare(run))


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, RuntimeError, ValueError, KeyError) as error:
        print(f"Incomplete or invalid audit: {error}", file=sys.stderr)
        sys.exit(2)
