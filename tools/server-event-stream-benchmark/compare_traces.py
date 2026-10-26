#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Compare request wall timings decoded from Dial9 binaries, without rerunning a benchmark."""
import argparse
from collections import defaultdict
import hashlib
import json
import math
from pathlib import Path
import re
import statistics
import subprocess

HERE = Path(__file__).resolve().parent
FIELDS = ("response_ready_ns", "first_frame_ready_ns", "body_complete_ns")


def digest(path):
    checksum = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            checksum.update(chunk)
    return checksum.hexdigest()


def integer(value):
    if isinstance(value, bool) or not (
        isinstance(value, int) or isinstance(value, str) and value.isdecimal()
    ):
        raise ValueError(f"Expected a nonnegative integer, got {value!r}")
    number = int(value)
    if number < 0:
        raise ValueError(f"Negative request timing: {number}")
    return number


def distribution(values):
    values = sorted(values)
    if not values:
        raise ValueError("No completed requests")
    result = {"samples": len(values), "mean_ns": statistics.mean(values), "max_ns": values[-1]}
    for percentile in (50, 95, 99):
        result[f"p{percentile}_ns"] = values[math.ceil(len(values) * percentile / 100) - 1]
    return result


def summarize_requests(requests, result):
    count = result["streams"]
    timing = result["server_timing"]
    if (len(requests) != count or any(timing[name] != count for name in
                                     ("requests_started", "responses_ready", "bodies_completed"))
            or any(timing[name] for name in ("active", "errors", "aborted"))
            or timing["statuses"] != {"200": count}):
        raise ValueError("Trace requests and successful measured streams do not match")
    values = {field: [] for field in FIELDS}
    for request in requests:
        if request.get("outcome") != "complete" or integer(request.get("status")) != 200:
            raise ValueError(f"Unsuccessful request in measured trace: {request}")
        elapsed = [integer(request[field]) for field in FIELDS]
        if elapsed != sorted(elapsed):
            raise ValueError(f"Request timings are out of order: {request}")
        for field, ns in zip(FIELDS, elapsed):
            values[field].append(ns)
    distributions = {field: distribution(ns) for field, ns in values.items()}
    # These are raw span values. HDR histograms are only an independent check;
    # three significant digits can round the recorded values upward by <0.1%.
    for field, actual in distributions.items():
        histogram = timing[field.removesuffix("_ns")]
        if histogram["samples"] != count:
            raise ValueError(f"Histogram count mismatch for {field}")
        for statistic in ("p50_ns", "p95_ns", "p99_ns", "max_ns"):
            recorded = histogram[statistic]
            if abs(actual[statistic] - recorded) > max(1, math.ceil(recorded * 0.001)):
                raise ValueError(f"Raw trace and HDR histogram disagree for {field}/{statistic}")
    return distributions


def segments(case, result):
    files = []
    for artifact in result["dial9"]["artifacts"]:
        path = case / Path(artifact["artifact"]).name
        match = re.search(r"\.(\d+)\.bin$", path.name)
        if not match or not path.is_file():
            raise ValueError(f"Missing numbered trace segment: {path}")
        if path.stat().st_size != artifact["bytes"] or digest(path) != artifact["sha256"]:
            raise ValueError(f"Trace size or checksum mismatch: {path}")
        files.append((int(match[1]), path))
    files.sort()
    if [index for index, _ in files] != list(range(len(files))) or not files:
        raise ValueError(f"Missing or duplicate trace segments in {case}")
    return [path for _, path in files]


def configuration(result, provenance):
    args, generation = provenance["args"], provenance["generation"]
    build = provenance["build"]
    return {
        **{name: result[name] for name in
           ("protocol", "direction", "kind", "size", "events_per_stream", "concurrency",
            "load_model", "offered_streams_per_second", "duplex_timestamp_window", "deep_validation")},
        "measurement_seconds": result["arrival_window_seconds"],
        **{name: args[name] for name in ("warmup", "server_workers", "server_cpus", "client_cpus")},
        "host": provenance["uname"],
        "comparison": generation["comparison"],
        "source_hashes": {side: generation[side]["source_hash"] for side in ("legacy", "schema")},
        "generation_hash": generation["generation_hash"],
        "build_hash": hashlib.sha256(json.dumps(build, sort_keys=True).encode()).hexdigest(),
    }


def comparisons(cases):
    groups = defaultdict(lambda: defaultdict(lambda: defaultdict(list)))
    for case in cases:
        key = json.dumps(case["configuration"], sort_keys=True)
        groups[key][case["run"]][case["side"]].append(case)
    output = []
    for key, runs in sorted(groups.items()):
        pairs = []
        for run, sides in sorted(runs.items()):
            if set(sides) != {"legacy", "schema"} or len(sides["legacy"]) != len(sides["schema"]):
                raise ValueError(f"Unmatched legacy/schema cases in {run}: {key}")
            # Pair repeated diagnostics in their case-directory order. Requests
            # from independent servers are not paired observations.
            for legacy, schema in zip(
                sorted(sides["legacy"], key=lambda c: c["case"]),
                sorted(sides["schema"], key=lambda c: c["case"]),
            ):
                ratios = {}
                for field in FIELDS:
                    ratios[field] = {}
                    for percentile in (50, 95, 99):
                        name = f"p{percentile}_ns"
                        denominator = legacy["distributions"][field][name]
                        numerator = schema["distributions"][field][name]
                        ratios[field][name] = numerator / denominator if denominator else None
                pairs.append({
                    "run": run, "legacy_case": legacy["case"], "schema_case": schema["case"],
                    "legacy_requests": legacy["requests"], "schema_requests": schema["requests"],
                    "legacy": legacy["distributions"], "schema": schema["distributions"],
                    "schema_over_legacy": ratios,
                })
        summary = {}
        for field in FIELDS:
            summary[field] = {}
            for percentile in (50, 95, 99):
                name = f"p{percentile}_ns"
                ratios = [pair["schema_over_legacy"][field][name] for pair in pairs]
                valid = [ratio for ratio in ratios if ratio is not None]
                summary[field][name] = {
                    "case_pairs": len(valid),
                    "median_schema_over_legacy": statistics.median(valid) if valid else None,
                    "min": min(valid) if valid else None, "max": max(valid) if valid else None,
                }
        output.append({"configuration": json.loads(key), "pairs": pairs, "summary": summary})
    return output


def report(groups):
    text = [
        "# Request timing comparison from decoded Dial9 traces", "",
        "All percentiles below are computed from completed `server_request` span fields "
        "decoded from binary traces. Artifact sizes, SHA-256, request/status counts, "
        "elapsed-time ordering and agreement with the separate HDR histograms were verified.", "",
        "`schema / legacy` is a latency ratio: values below 1 mean lower schema latency. "
        "These are wall durations from HTTP service entry through EOS handed to Hyper, "
        "including backpressure and tracing overhead. They are not CPU or pure serde costs.", "",
        "Configurations and builds are matched within each pair. Different stream lengths, "
        "concurrency, durations, affinity, hosts or builds are kept separate. Across compatible "
        "runs, summaries weight case-pair ratios equally rather than pooling request samples. "
        "Requests are not independent benchmark repetitions; no request-level confidence "
        "interval or general performance claim is inferred.", "",
        "| Run | Protocol | Direction | Event | Events/stream | Concurrency | Requests legacy/schema | "
        "Complete p50 legacy ms | Schema ms | Schema/legacy | Complete p95 legacy/schema ms | "
        "Complete p99 legacy/schema ms |",
        "|---|---|---|---|---:|---:|---|---:|---:|---:|---|---|",
    ]
    for group in groups:
        config = group["configuration"]
        for pair in group["pairs"]:
            legacy = pair["legacy"]["body_complete_ns"]
            schema = pair["schema"]["body_complete_ns"]
            ratio = pair["schema_over_legacy"]["body_complete_ns"]["p50_ns"]
            ratio_text = f"{ratio:.4f}" if ratio is not None else "undefined"
            text.append(
                f'| {Path(pair["run"]).name} | {config["protocol"]} | {config["direction"]} | '
                f'{config["kind"]} | {config["events_per_stream"]} | {config["concurrency"]} | '
                f'{pair["legacy_requests"]}/{pair["schema_requests"]} | '
                f'{legacy["p50_ns"] / 1e6:.4f} | {schema["p50_ns"] / 1e6:.4f} | {ratio_text} | '
                f'{legacy["p95_ns"] / 1e6:.4f}/{schema["p95_ns"] / 1e6:.4f} | '
                f'{legacy["p99_ns"] / 1e6:.4f}/{schema["p99_ns"] / 1e6:.4f} |'
            )
    text += ["", "The JSON report includes response-ready and first-frame percentiles, all "
             "per-case distributions, compatible-run summaries and source provenance. "
             "Decoded per-request JSON lines are retained under `requests/`.", ""]
    return "\n".join(text)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("runs", type=Path, nargs="+", help="Run directories containing numbered cases")
    parser.add_argument("--decoder", type=Path,
                        default=HERE / "artifacts/trace-reader/release/benchmark-trace-reader")
    parser.add_argument("--output", type=Path, required=True, help="New analysis directory")
    args = parser.parse_args()
    decoder = args.decoder.resolve()
    if not decoder.is_file():
        parser.error("Build the trace reader first; see README 'Comparing request timings from traces'.")
    runs = [run.resolve() for run in args.runs]
    if len(set(runs)) != len(runs):
        parser.error("Duplicate run directories")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    (output / "requests").mkdir()
    cases = []
    try:
        for run_index, run in enumerate(runs):
            provenance = json.loads((run / "provenance.json").read_text())
            found = 0
            for path in sorted(run.glob("*/result.json")):
                result = json.loads(path.read_text())
                if result.get("mode") != "dial9" or "preflight" in result:
                    continue
                if result["side"] not in ("legacy", "schema"):
                    raise ValueError(f"Unknown variant in {path}")
                files = segments(path.parent, result)
                process = subprocess.run([str(decoder), *map(str, files)],
                                         text=True, capture_output=True, check=True)
                requests = [json.loads(line) for line in process.stdout.splitlines()]
                distributions = summarize_requests(requests, result)
                decoded = output / "requests" / f"{run_index:02d}-{path.parent.name}.jsonl"
                decoded.write_text(process.stdout)
                cases.append({
                    "run": str(run), "case": path.parent.name, "side": result["side"],
                    "requests": len(requests), "decoded_requests": str(decoded),
                    "trace_artifacts": result["dial9"]["artifacts"],
                    "configuration": configuration(result, provenance),
                    "distributions": distributions,
                })
                found += 1
                print(f'{run.name}/{path.parent.name}: {result["side"]}, '
                      f'{len(requests)} verified trace requests', flush=True)
            if not found:
                raise ValueError(f"No measured Dial9 cases in {run}")
        groups = comparisons(cases)
        data = {"definition": "raw Dial9 request elapsed nanoseconds; schema/legacy <1 is lower latency",
                "cases": cases, "comparisons": groups,
                "case_count": len(cases), "request_count": sum(case["requests"] for case in cases)}
        (output / "comparison.json").write_text(json.dumps(data, indent=2) + "\n")
        (output / "report.md").write_text(report(groups))
        print(f'Report: {output / "report.md"}', flush=True)
    except Exception as error:
        (output / "FAILED.txt").write_text(f"{type(error).__name__}: {error}\n")
        raise


if __name__ == "__main__":
    main()
