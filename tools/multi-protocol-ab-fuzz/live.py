# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Live HTTP A/B comparison of the generated servers, in three tiers.

Tier `single`  — generated serde single-protocol server vs schema-serde single-protocol server
                 (same one protocol on both sides). Pure serde comparison: no routing
                 ambiguity, every divergence is a finding.
Tier `bunch`   — generated serde single-protocol servers vs schema servers serving non-conflicting
                 protocol sets (never awsJson1_0 with awsJson1_1, never restJson1 with
                 restXml): bunch-json = awsJson1_0 + restJson1 + rpcv2Cbor,
                 bunch-xml = awsJson1_1 + restXml + rpcv2Cbor.
Tier `full`    — generated serde singles vs one server serving all five protocols (plus the
                 no-restJson1 full-xml server for the rest-xml-nojson comparison).

    source env.sh && FUZZ_ROOT=$AB_FUZZ_DIR/artifacts-live python3 live.py run
    python3 live.py run --tiers single --suites pokemon

Requires the three generation passes (see AB_FUZZING.md): the baseline `single` side, the
schema side with MP_FUZZ_ISOLATE_PROTOCOLS=true (schema-server + ab-harness per protocol),
and the schema side with MP_FUZZ_VARIANTS defining full, full-xml, bunch-json and bunch-xml.

Each side runs as its own process via `aws-smithy-fuzz serve` and is verified through
`GET /__ab/build` before any comparison. Port plan: suite blocks 1000 apart from 18000;
before servers at base + 10*i, tier-specific after servers at base+100 and up.
"""
import argparse
import hashlib
import http.client
import json
import os
import struct
import subprocess
import sys
import time
import xml.etree.ElementTree as ET
from pathlib import Path

import seeds as seedlib

AB_FUZZ_DIR = Path(__file__).resolve().parent
FUZZ_ROOT = Path(os.environ.get("FUZZ_ROOT", AB_FUZZ_DIR / "artifacts"))
GEN = FUZZ_ROOT / "gen"
LIVE = FUZZ_ROOT / "live"
DRIVER = FUZZ_ROOT / "driver" / "bin" / "aws-smithy-fuzz"
BASELINE_REPO = Path(os.environ.get("BASELINE_REPO",
                                    os.path.realpath(Path.home() / "smithy-rs-latest")))
CURRENT_REPO = Path(os.environ.get("CURRENT_REPO", AB_FUZZ_DIR.parent.parent))

SUITE_BASES = {"multiprotocol": 18000, "pokemon": 19000}
CASES = ["aws-json-10", "aws-json-11", "rest-json1", "rest-xml", "rpcv2-cbor"]
BUNCHES = {
    "bunch-json": ["aws-json-10", "rest-json1", "rpcv2-cbor"],
    "bunch-xml": ["aws-json-11", "rest-xml", "rpcv2-cbor"],
}
PROTOCOL_IDS = {
    "aws-json-10": "aws.protocols#awsJson1_0",
    "aws-json-11": "aws.protocols#awsJson1_1",
    "rest-json1": "aws.protocols#restJson1",
    "rest-xml": "aws.protocols#restXml",
    "rpcv2-cbor": "smithy.protocols#rpcv2Cbor",
}


def sh(cmd, cwd=None, log=None):
    out = open(log, "ab") if log else subprocess.DEVNULL
    try:
        subprocess.run(cmd, cwd=cwd, check=True, stdout=out, stderr=out)
    finally:
        if log:
            out.close()


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def git(repo, *args):
    return subprocess.run(["git", "-C", str(repo), *args], check=True,
                          capture_output=True, text=True).stdout.strip()


def verify_side(side, server_crate, repo):
    """AB_FUZZING step 4: check the compiled serde implementation and runtime paths."""
    lib = (Path(server_crate) / "src" / "lib.rs").read_text()
    protocol_serde = "mod protocol_serde" in lib
    schema = "mod schema" in lib
    cargo = (Path(server_crate) / "Cargo.toml").read_text()
    candidates = {str(repo), str(repo.resolve()), os.path.realpath(repo)}
    if not any(f"{c}/rust-runtime" in cargo for c in candidates):
        raise SystemExit(f"{server_crate}: runtime paths do not resolve to {repo}")
    if side == "before" and not protocol_serde:
        raise SystemExit(f"{server_crate}: before server lacks the legacy protocol_serde module")
    if side == "after" and not schema:
        raise SystemExit(f"{server_crate}: after server lacks the schema module")
    return {"protocol_serde_module": protocol_serde, "schema_module": schema}


BUILT = {}


def build_cdylib(harness_crate, name, log, keep_as):
    harness_crate = Path(harness_crate)
    if harness_crate in BUILT:
        return BUILT[harness_crate]
    sh(["cargo", "build", "--release"], cwd=harness_crate, log=log)
    # A .cargo config higher up the tree can redirect the target dir; ask cargo where it is.
    metadata = subprocess.run(["cargo", "metadata", "--format-version", "1", "--no-deps"],
                              cwd=harness_crate, check=True, capture_output=True, text=True)
    target = Path(json.loads(metadata.stdout)["target_directory"])
    lib = target / "release" / f"lib{name.replace('-', '_')}.so"
    if not lib.exists():
        raise SystemExit(f"missing {lib}")
    keep_as.parent.mkdir(parents=True, exist_ok=True)
    keep_as.write_bytes(lib.read_bytes())
    BUILT[harness_crate] = keep_as
    return keep_as


def probe(port, path="/__ab/build"):
    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
    conn.request("GET", path)
    response = conn.getresponse()
    data = response.read()
    conn.close()
    if response.status != 200:
        raise SystemExit(f"port {port}: probe returned {response.status}")
    return json.loads(data)


def ensure_port_free(port):
    try:
        probe(port)
    except (ConnectionRefusedError, OSError):
        return
    raise SystemExit(f"port {port} already answers; refusing to compare against a stale process")


def launch(label, lib, port, build_info, log):
    info_path = log.parent / f"build-info-{label.replace('/', '-')}.json"
    info_path.write_text(json.dumps(build_info, indent=2))
    ensure_port_free(port)
    process = subprocess.Popen(
        [str(DRIVER), "serve", "--shared-library-path", str(lib),
         "--address", f"127.0.0.1:{port}", "--build-info", str(info_path)],
        stdout=open(log, "ab"), stderr=subprocess.STDOUT)
    for _ in range(150):
        try:
            reported = probe(port)
            break
        except (ConnectionRefusedError, OSError):
            if process.poll() is not None:
                raise SystemExit(f"{label}: serve exited, see {log}")
            time.sleep(0.2)
    else:
        raise SystemExit(f"{label}: no answer on port {port}")
    if reported.get("build_id") != build_info["build_id"]:
        raise SystemExit(f"port {port}: build id mismatch (stale process?)")
    return process


def send(port, method, uri, headers, body):
    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=30)
    conn.putrequest(method, uri, skip_host=True, skip_accept_encoding=True)
    conn.putheader("host", f"127.0.0.1:{port}")
    for name, values in headers.items():
        for value in values:
            conn.putheader(name, value)
    if body or method not in ("GET", "HEAD"):
        conn.putheader("content-length", str(len(body)))
    conn.endheaders()
    if body:
        conn.send(body)
    response = conn.getresponse()
    out = {"status": response.status,
           "headers": {k.lower(): v for k, v in response.getheaders()},
           "body": response.read()}
    conn.close()
    return out


def decode_cbor(data):
    """Minimal CBOR decode to python values; definite and indefinite containers compare equal."""
    def item(i):
        initial = data[i]
        major, info = initial >> 5, initial & 0x1F
        i += 1
        if info < 24:
            arg = info
        elif info in (24, 25, 26, 27):
            width = 1 << (info - 24)
            arg = int.from_bytes(data[i:i + width], "big")
            i += width
        elif info == 31:
            arg = None  # indefinite
        else:
            raise ValueError("reserved")
        if major == 0:
            return arg, i
        if major == 1:
            return -1 - arg, i
        if major in (2, 3):
            if arg is None:
                chunks = []
                while data[i] != 0xFF:
                    chunk, i = item(i)
                    chunks.append(chunk)
                joined = (b"" if major == 2 else "").join(chunks)
                return joined, i + 1
            raw = data[i:i + arg]
            i += arg
            return raw if major == 2 else raw.decode(), i
        if major == 4:
            out = []
            if arg is None:
                while data[i] != 0xFF:
                    value, i = item(i)
                    out.append(value)
                return out, i + 1
            for _ in range(arg):
                value, i = item(i)
                out.append(value)
            return out, i
        if major == 5:
            out = {}
            if arg is None:
                while data[i] != 0xFF:
                    key, i = item(i)
                    value, i = item(i)
                    out[str(key)] = value
                return out, i + 1
            for _ in range(arg):
                key, i = item(i)
                value, i = item(i)
                out[str(key)] = value
            return out, i
        if major == 6:
            return item(i)  # drop the tag
        if info == 20:
            return False, i
        if info == 21:
            return True, i
        if info in (22, 23):
            return None, i
        # The generic header parse above already consumed the argument bytes.
        if info == 25:
            return struct.unpack(">e", arg.to_bytes(2, "big"))[0], i
        if info == 26:
            return struct.unpack(">f", arg.to_bytes(4, "big"))[0], i
        if info == 27:
            return struct.unpack(">d", arg.to_bytes(8, "big"))[0], i
        return f"<simple:{arg}>", i

    value, end = item(0)
    if end != len(data):
        raise ValueError("trailing bytes")
    return value


def canonical_xml(element):
    children = sorted((canonical_xml(child) for child in element), key=repr)
    return (element.tag, (element.text or "").strip(), tuple(sorted(element.attrib.items())),
            tuple(children))


IGNORED_HEADERS = {"content-length", "connection", "date"}


def normalize(response):
    headers = {k: v for k, v in response["headers"].items() if k not in IGNORED_HEADERS}
    content_type = headers.get("content-type", "")
    body = response["body"]
    try:
        if "json" in content_type:
            body = json.dumps(json.loads(body or b"{}"), sort_keys=True)
        elif "cbor" in content_type:
            body = repr(decode_cbor(body)) if body else ""
        elif "xml" in content_type:
            body = repr(canonical_xml(ET.fromstring(body.decode()))) if body else ""
        else:
            body = repr(body)
    except (ValueError, UnicodeDecodeError, ET.ParseError, IndexError, struct.error):
        body = repr(body)
    return {"status": response["status"], "headers": headers, "body": body}


def classify(before, after):
    if normalize(before) == normalize(after):
        return "EQUAL"
    # Both sides reject before a handler runs, with different framing.
    if 400 <= before["status"] < 500 and 400 <= after["status"] < 500:
        return "BOTH_REJECTED"
    return "DIFF"


def run_comparison(suite, protocol, label, before_port, after_port, report_dir):
    results = {"EQUAL": 0, "BOTH_REJECTED": 0, "DIFF": 0, "ERROR": 0}
    findings = []
    for seed, uri, method, headers, body in seedlib.seeds(suite, protocol):
        try:
            before = send(before_port, method, uri, headers, body)
            after = send(after_port, method, uri, headers, body)
        except Exception as error:  # noqa: BLE001 - record and continue
            results["ERROR"] += 1
            findings.append({"seed": seed, "error": repr(error)})
            continue
        verdict = classify(before, after)
        results[verdict] += 1
        if verdict != "EQUAL":
            findings.append({"seed": seed, "verdict": verdict,
                             "before": normalize(before), "after": normalize(after)})
    (report_dir / f"{label}.json").write_text(json.dumps(
        {"suite": suite, "comparison": label, "results": results, "findings": findings},
        indent=2, default=repr))
    return results


class Launcher:
    def __init__(self, revisions):
        self.revisions = revisions
        self.processes = []

    def start(self, side, suite, label, harness_crate, server_crate, protocols, port,
              report_dir):
        repo = BASELINE_REPO if side == "before" else CURRENT_REPO
        checks = verify_side(side, server_crate, repo)
        target = "fuzz-target-single" if side == "before" else "fuzz-target-multi"
        lib = build_cdylib(harness_crate, target, report_dir / f"build-{label}.log",
                           report_dir / f"lib-{label}.so")
        info = {"side": side, "suite": suite, "schema_serde": side == "after",
                "protocols": [PROTOCOL_IDS[p] for p in protocols],
                "generator": self.revisions[side], **checks,
                "shared_library_sha256": sha256(lib),
                "build_id": f"{suite}-{label}-{sha256(lib)[:12]}"}
        self.processes.append(launch(f"{suite}/{label}", lib, port, info,
                                     report_dir / f"serve-{label}.log"))

    def stop(self):
        for process in self.processes:
            process.terminate()
        for process in self.processes:
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
        self.processes = []


def run_tier(tier, suite, cases, revisions, summary):
    base = SUITE_BASES[suite]
    report_dir = LIVE / tier / suite
    report_dir.mkdir(parents=True, exist_ok=True)
    suite_dir = GEN / suite
    launcher = Launcher(revisions)
    try:
        before_ports = {}
        for i, case in enumerate(cases):
            before_ports[case] = base + 10 * i
            launcher.start("before", suite, f"before-{case}",
                           suite_dir / case / "ab-harness" / "single",
                           suite_dir / case / "single-server",
                           [case], before_ports[case], report_dir)

        if tier == "single":
            for i, case in enumerate(cases):
                after_port = base + 100 + 10 * i
                launcher.start("after", suite, f"after-{case}",
                               suite_dir / case / "ab-harness" / "multi",
                               suite_dir / case / "schema-server",
                               [case], after_port, report_dir)
                summary[f"{tier}:{suite}/{case}"] = run_comparison(
                    suite, case, case, before_ports[case], after_port, report_dir)
        elif tier == "bunch":
            bunch_ports = {}
            for i, (bunch, served) in enumerate(BUNCHES.items()):
                bunch_ports[bunch] = base + 120 + 10 * i
                launcher.start("after", suite, f"after-{bunch}",
                               suite_dir / f"{bunch}-harness" / "multi",
                               suite_dir / f"{bunch}-server",
                               served, bunch_ports[bunch], report_dir)
            for case in cases:
                for bunch, served in BUNCHES.items():
                    if case not in served:
                        continue
                    label = f"{case}@{bunch}"
                    summary[f"{tier}:{suite}/{label}"] = run_comparison(
                        suite, case, label, before_ports[case], bunch_ports[bunch], report_dir)
        elif tier == "full":
            launcher.start("after", suite, "after-full",
                           suite_dir / "full-harness" / "multi",
                           suite_dir / "full-server", CASES, base + 100, report_dir)
            launcher.start("after", suite, "after-full-xml",
                           suite_dir / "full-xml-harness" / "multi",
                           suite_dir / "full-xml-server",
                           [c for c in CASES if c != "rest-json1"], base + 110, report_dir)
            for case in cases:
                summary[f"{tier}:{suite}/{case}"] = run_comparison(
                    suite, case, case, before_ports[case], base + 100, report_dir)
            if "rest-xml" in cases:
                summary[f"{tier}:{suite}/rest-xml-nojson"] = run_comparison(
                    suite, "rest-xml", "rest-xml-nojson", before_ports["rest-xml"],
                    base + 110, report_dir)
    finally:
        launcher.stop()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=["run"])
    parser.add_argument("--tiers", nargs="*", default=["single", "bunch", "full"])
    parser.add_argument("--suites", nargs="*", default=["multiprotocol", "pokemon"])
    parser.add_argument("--cases", nargs="*", default=CASES)
    args = parser.parse_args()

    if not DRIVER.exists():
        raise SystemExit(f"driver missing at {DRIVER}; run generate.sh first")
    revisions = {
        "before": {"repo": str(BASELINE_REPO), "commit": git(BASELINE_REPO, "rev-parse", "HEAD"),
                   "dirty": bool(git(BASELINE_REPO, "status", "--porcelain"))},
        "after": {"repo": str(CURRENT_REPO), "commit": git(CURRENT_REPO, "rev-parse", "HEAD"),
                  "dirty": bool(git(CURRENT_REPO, "status", "--porcelain"))},
    }

    summary = {}
    for tier in args.tiers:
        for suite in args.suites:
            run_tier(tier, suite, args.cases, revisions, summary)

    print(json.dumps({"revisions": revisions, "summary": summary}, indent=2))
    failed = any(counts["DIFF"] or counts["ERROR"] for counts in summary.values())
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
