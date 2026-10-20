#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Measure already built binaries. Never pulls, generates, or compiles."""
import argparse
import csv
from datetime import datetime, timezone
import itertools
import json
import math
import os
from pathlib import Path
import selectors
import random
import shutil
import socket
import statistics
import subprocess
import threading
import time
from common import HERE, PROTOCOLS, cargo_env, digest, tool_hash, validate_generation, write_json


def line(process, timeout=60):
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    try:
        if not selector.select(timeout):
            raise RuntimeError(f"Process {process.pid} did not respond within {timeout}s")
        out = process.stdout.readline()
        if not out:
            raise RuntimeError(f"Process {process.pid} exited unexpectedly ({process.poll()})")
        return out.strip()
    finally:
        selector.close()


def control(process, text):
    process.stdin.write(text + "\n")
    process.stdin.flush()
    return json.loads(line(process))


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=10)


def rss(pid):
    try:
        for value in Path(f"/proc/{pid}/status").read_text().splitlines():
            if value.startswith("VmRSS:"):
                return int(value.split()[1])
    except FileNotFoundError:
        pass
    return None


def difference(before, after, events, elapsed):
    out = {"before": before, "after": after}
    for key in ("user_seconds", "system_seconds", "allocations", "allocated_bytes"):
        if key in before:
            out[key] = after[key] - before[key]
    out["cpu_seconds"] = out["user_seconds"] + out["system_seconds"]
    out["cpu_percent_of_one_core"] = 100 * out["cpu_seconds"] / elapsed
    out["cpu_ns_per_event"] = out["cpu_seconds"] * 1e9 / events
    for key in ("allocations", "allocated_bytes"):
        if key in out:
            out[f"{key}_per_event"] = out[key] / events
    return out


def binary(cache, protocol, role, mode):
    if mode == "latency":
        mode = "cpu"
    return cache / "runners" / protocol / role / f"target-{mode}" / "release" / f"stream-bench-{role}"


def validate_build(cache):
    validate_generation(cache)
    data = json.loads((cache / "build.json").read_text())
    if data["tool_hash"] != tool_hash():
        raise SystemExit("Benchmark code changed after compilation. Run build.py.")
    if data["generation_hash"] != digest(cache / "generation.json"):
        raise SystemExit("Generation provenance changed after compilation.")
    compiler = subprocess.check_output(["rustc", f'+{data["toolchain"]}', "-Vv"], env=cargo_env(), text=True)
    if compiler != data["compiler"]:
        raise SystemExit("Compiler changed after compilation.")
    for p, expected in {**data["binaries"], **data["locks"]}.items():
        if digest(cache / p) != expected:
            raise SystemExit(f"Cached binary or dependency lock changed: {p}")
    return data


def topology(cpus):
    groups = {}
    for cpu in cpus:
        path = Path(f"/sys/devices/system/cpu/cpu{cpu}/topology/thread_siblings_list")
        if not path.exists():
            raise SystemExit(f"CPU {cpu} is unavailable")
        key = path.read_text().strip()
        if key in groups:
            raise SystemExit(f"CPUs {cpu} and {groups[key]} share a physical core")
        groups[key] = cpu
    if not set(cpus) <= os.sched_getaffinity(0):
        raise SystemExit("Requested CPUs are outside the process's allowed affinity")
    return groups


def perf_start(kind, pid, dest):
    log = (dest / f"perf-{kind}-{pid}.log").open("w")
    out = dest / f"perf-{kind}-{pid}"
    if kind == "stat":
        command = ["perf", "stat", "-x", ",", "-o", str(out) + ".csv", "-e", "task-clock,cycles:u,instructions:u,context-switches,page-faults", "-p", str(pid)]
    else:
        command = ["perf", "record", "-e", "cycles:u", "-m", "32", "-g", "--call-graph", "fp", "-F", "99", "-o", str(out) + ".data", "-p", str(pid)]
    proc = subprocess.Popen(command, stdout=log, stderr=log)
    return proc, log, command, out


def counters(path, events):
    """Preserve perf's unavailable/scaled status and normalize supported values."""
    out = {}
    for line in path.read_text().splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        fields = line.split(",")
        if len(fields) < 3:
            continue
        value, unit, name = (f.strip() for f in fields[:3])
        try:
            number = float(value)
        except ValueError:
            out[name] = {"unavailable": value, "unit": unit}
            continue
        out[name] = {"value": number, "unit": unit, "per_event": number / events, "raw_fields": fields}
    return out


def perf_finish(profiles):
    results = []
    stopped = set()
    for proc, log, command, path in profiles:
        if proc.poll() is None:
            stopped.add(proc.pid)
            proc.send_signal(2)
    for proc, log, command, path in profiles:
        if proc.pid in stopped:
            try:
                proc.wait(timeout=15)
            except subprocess.TimeoutExpired:
                stop(proc)
        log.close()
        result = {"command": command, "exit_code": proc.returncode, "stopped_by_controller": proc.pid in stopped, "artifact": str(path)}
        data = Path(str(path) + ".data")
        if data.exists() and data.stat().st_size:
            for cmd, suffix in ((["perf", "script", "-i", str(data)], ".stacks.txt"), (["perf", "report", "--stdio", "-i", str(data)], ".hotspots.txt")):
                with Path(str(path) + suffix).open("w") as f:
                    subprocess.run(cmd, stdout=f, stderr=subprocess.STDOUT, check=False)
            collapse, render = shutil.which("stackcollapse-perf.pl"), shutil.which("flamegraph.pl")
            if collapse and render:
                folded = Path(str(path) + ".folded")
                with Path(str(path) + ".stacks.txt").open() as src, folded.open("w") as dst:
                    subprocess.run([collapse], stdin=src, stdout=dst, check=True)
                with folded.open() as src, Path(str(path) + ".svg").open("w") as dst:
                    subprocess.run([render], stdin=src, stdout=dst, check=True)
        results.append(result)
    return results


def case(cache, dest, protocol, side, direction, kind, size, count, concurrency, warmup, seconds, mode, server_cpus, client_cpus, workers, preflight=False, profile=None, stream_rate=0, duplex_window=None, deep_validation=False):
    dest.mkdir(parents=True, exist_ok=False)
    server = client = None
    profiles = []
    sampler = None
    done = threading.Event()
    sampling = []
    commands = []
    logs = []
    result = None
    port = None
    try:
        server_command = ["taskset", "-c", server_cpus, str(binary(cache, protocol, side, mode)), str(workers), str(preflight or deep_validation).lower()]
        commands.append(server_command)
        log = (dest / "server.stderr").open("w")
        logs.append(log)
        server = subprocess.Popen(server_command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, text=True, bufsize=1)
        ready = line(server)
        if not ready.startswith("READY:"):
            raise RuntimeError(f"Unexpected server readiness: {ready}")
        port = int(ready.split(":")[1])
        endpoint = f"http://127.0.0.1:{port}"
        # Retain proof of the actual bind, not only the source's configured address.
        listen = subprocess.check_output(["ss", "-ltnp"], text=True)
        matching = [s for s in listen.splitlines() if f"127.0.0.1:{port}" in s]
        if not matching:
            raise RuntimeError(f"Missing loopback listener on {port}")
        (dest / "listener.txt").write_text("\n".join(matching) + "\n")
        latency = mode == "latency" or preflight
        window = duplex_window or count
        client_command = ["taskset", "-c", client_cpus, str(binary(cache, protocol, "client", mode)), endpoint, direction, kind, str(size), str(count), str(concurrency), str(warmup), str(seconds), str(preflight).lower(), str(stream_rate), str(latency).lower(), str(window), str(deep_validation).lower()]
        commands.append(client_command)
        log = (dest / "client.stderr").open("w")
        logs.append(log)
        env = {**os.environ, "BENCH_CLIENT_WORKERS": str(len(client_cpus.split(",")))}
        client = subprocess.Popen(client_command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, text=True, bufsize=1, env=env)
        if preflight:
            result = json.loads(line(client, 60))
            if result["preflight"] != "passed":
                raise RuntimeError("Preflight failed")
        else:
            if line(client, 60 + warmup) != "WARM":
                raise RuntimeError("Missing warmup barrier")
            if profile and shutil.which("perf"):
                for process in (server, client):
                    profiles.append(perf_start(profile, process.pid, dest))
                time.sleep(0.3)  # attach before timing
            before = control(server, "reset")
            idle = {"server": rss(server.pid), "client": rss(client.pid)}

            def sample():
                while not done.is_set():
                    sampling.append({"monotonic": time.monotonic(), "server_rss_kib": rss(server.pid), "client_rss_kib": rss(client.pid)})
                    done.wait(0.1)

            sampler = threading.Thread(target=sample)
            sampler.start()
            client.stdin.write("go\n")
            client.stdin.flush()
            result = json.loads(line(client, seconds + 120))
            after = control(server, "snapshot")
            done.set()
            sampler.join(timeout=2)
            events, elapsed = result["events"], result["elapsed_seconds"]
            if events != result["streams"] * count:
                raise RuntimeError("Client event accounting does not match completed streams")
            if stream_rate and result["streams"] != math.ceil(stream_rate * seconds):
                raise RuntimeError("Scheduled arrivals were omitted from completed-stream accounting")
            if latency and result["completion"]["samples"] != result["streams"]:
                raise RuntimeError("Latency histogram omitted completed streams")
            result["server"] = difference(before, after, events, elapsed)
            result["client"] = difference(result.pop("before"), result.pop("after"), events, elapsed)
            result["rss"] = {"idle_kib": idle, "samples": len(sampling), "interval_seconds": 0.1}
            for role in ("server", "client"):
                values = [s[f"{role}_rss_kib"] for s in sampling if s[f"{role}_rss_kib"] is not None]
                result["rss"][f"{role}_sampled_peak_kib"] = max(values) if values else None
            write_json(dest / "rss-samples.json", sampling)
            if profiles:
                result["perf"] = perf_finish(profiles)
                for role, profile_result in zip(("server", "client"), result["perf"]):
                    profile_result["role"] = role
                    csv_path = Path(profile_result["artifact"] + ".csv")
                    if csv_path.exists():
                        profile_result["counters"] = counters(csv_path, events)
                profiles = []
            elif profile:
                result["perf"] = {"unavailable": "perf executable is unavailable"}
            client.stdin.write("done\n")
            client.stdin.flush()
        if client.wait(timeout=10) != 0:
            raise RuntimeError("Client failed")
        result.update({"protocol": protocol, "side": side, "direction": direction, "kind": kind, "size": size, "events_per_stream": count, "concurrency": concurrency, "mode": mode, "profile": profile})
        write_json(dest / "result.json", result)
        return result
    finally:
        done.set()
        if sampler:
            sampler.join(timeout=2)
        if profiles:
            perf_finish(profiles)
        for process in (client, server):
            if process:
                stop(process)
        for log in logs:
            log.close()
        write_json(dest / "commands.json", commands)
        if server:
            write_json(dest / "cleanup.json", {"server_exit": server.poll(), "client_exit": client.poll() if client else None, "port": port})
        if port:
            with socket.socket() as probe:
                probe.settimeout(1)
                if probe.connect_ex(("127.0.0.1", port)) == 0:
                    raise RuntimeError(f"Benchmark listener {port} is still open")


def paired_interval(ratios):
    """Seeded percentile bootstrap of the geometric mean paired ratio.

    Ten independent pairs is a minimum reporting gate, not a guarantee of
    sufficient precision. Thermal drift and correlated runs can invalidate the
    interval's interpretation; raw pairs and run order remain available.
    """
    logs = [math.log(r) for r in ratios]
    estimate = math.exp(statistics.mean(logs))
    if len(logs) < 10:
        return {"geometric_mean": estimate, "bootstrap_95_interval": None,
                "reason": "Fewer than ten pairs; descriptive results only"}
    rng = random.Random(0)
    samples = sorted(math.exp(statistics.mean(rng.choices(logs, k=len(logs)))) for _ in range(5000))
    return {"geometric_mean": estimate, "bootstrap_95_interval": [samples[125], samples[4874]],
            "resamples": 5000, "seed": 0, "assumption": "independent paired repetitions"}


def summary(rows):
    cpu = [r for r in rows if r["mode"] == "cpu"]
    keys = ("protocol", "direction", "kind", "size", "concurrency")
    grouped = {}
    for row in cpu:
        grouped.setdefault(tuple(row[k] for k in keys), {}).setdefault(row["side"], []).append(row)
    results = []
    for key, sides in sorted(grouped.items()):
        item = dict(zip(keys, key))
        pairs = []
        assert set(sides) == {"legacy", "schema"}
        assert len(sides["legacy"]) == len(sides["schema"])
        assert len({r["pair"] for r in sides["legacy"]}) == len(sides["legacy"])
        assert len({r["pair"] for r in sides["schema"]}) == len(sides["schema"])
        for legacy, schema in zip(sorted(sides["legacy"], key=lambda r: r["pair"]), sorted(sides["schema"], key=lambda r: r["pair"])):
            assert legacy["pair"] == schema["pair"]
            pairs.append(schema["events_per_second"] / legacy["events_per_second"])
        item["paired_throughput_ratio"] = {"median": statistics.median(pairs), "min": min(pairs), "max": max(pairs), "pairs": len(pairs)}
        item["paired_uncertainty"] = paired_interval(pairs)
        for side, values in sides.items():
            item[side] = {
                "events_per_second_median": statistics.median(r["events_per_second"] for r in values),
                "server_cpu_ns_per_event_median": statistics.median(r["server"]["cpu_ns_per_event"] for r in values),
                "client_cpu_ns_per_event_median": statistics.median(r["client"]["cpu_ns_per_event"] for r in values),
            }
        results.append(item)
    return results


def report(dest, rows, comparison="unspecified"):
    data = summary(rows)
    write_json(dest / "summary.json", data)
    with (dest / "summary.csv").open("w", newline="") as f:
        fields = ["protocol", "direction", "kind", "size", "concurrency", "legacy_events_s", "schema_events_s", "paired_ratio_median", "paired_ratio_min", "paired_ratio_max", "pairs"]
        writer = csv.DictWriter(f, fieldnames=fields)
        writer.writeheader()
        for r in data:
            writer.writerow({**{k: r[k] for k in fields[:5]}, "legacy_events_s": r["legacy"]["events_per_second_median"], "schema_events_s": r["schema"]["events_per_second_median"], **{f"paired_ratio_{k}": r["paired_throughput_ratio"][k] for k in ("median", "min", "max")}, "pairs": r["paired_throughput_ratio"]["pairs"]})
    text = [
        "# Generated client/server event-stream comparison\n",
        f"Comparison: {comparison}. `legacy` labels identify generated serde. "
        + ("Both servers and the shared client come from the same source tree; server schemaSerde differs."
           if comparison == "same-revision" else "Different revisions can confound serialization comparisons.")
        + " Application MiB/s counts text/blob bytes, excluding framing, headers and structured metadata; duplex counts completed round trips. CPU, sampled RSS, requested heap, and latency/profiling are separate observations.\n",
        "| Protocol | Direction | Event | Bytes | Concurrency | Legacy events/s | Schema events/s | Paired schema/legacy (median, range) |",
        "|---|---|---|---:|---:|---:|---:|---|",
    ]
    for r in data:
        ratio = r["paired_throughput_ratio"]
        text.append(f'| {r["protocol"]} | {r["direction"]} | {r["kind"]} | {r["size"]} | {r["concurrency"]} | {r["legacy"]["events_per_second_median"]:.1f} | {r["schema"]["events_per_second_median"]:.1f} | {ratio["median"]:.3f} ({ratio["min"]:.3f}–{ratio["max"]:.3f}), n={ratio["pairs"]} |')
    text += ["", "Raw result.json files retain accounting and separate latency passes. Allocation, latency and perf passes do not contribute to throughput ratios. No runs are discarded; the observed range is not a confidence interval. Seeded paired bootstrap intervals estimate the geometric mean ratio, assume independent pairs, and are withheld below ten pairs. They do not establish machine isolation or client headroom.", "",
             "| Protocol | Direction | Event | Bytes | Concurrency | Geometric mean ratio | Bootstrap 95% interval |",
             "|---|---|---|---:|---:|---:|---|"]
    for r in data:
        uncertainty = r["paired_uncertainty"]
        interval = uncertainty["bootstrap_95_interval"]
        display = f"{interval[0]:.3f}–{interval[1]:.3f}" if interval else "insufficient pairs"
        text.append(f'| {r["protocol"]} | {r["direction"]} | {r["kind"]} | {r["size"]} | {r["concurrency"]} | {uncertainty["geometric_mean"]:.3f} | {display} |')
    text += ["", "Separate latency runs:", "",
             "| Protocol | Direction | Event | Concurrency | Side | Load model | Offered streams/s | Streams | Dispatch p99 ms | Completion p99 ms | Drain s |",
             "|---|---|---|---:|---|---|---:|---:|---:|---:|---:|"]
    for r in rows:
        if r["mode"] != "latency":
            continue
        text.append(f'| {r["protocol"]} | {r["direction"]} | {r["kind"]} | {r["concurrency"]} | {r["side"]} | {r["load_model"]} | {r["offered_streams_per_second"] or "unpaced"} | {r["streams"]} | {r["dispatch_delay"]["p99_ns"] / 1e6:.3f} | {r["completion"]["p99_ns"] / 1e6:.3f} | {r["drain_seconds"]:.3f} |')
    text += ["", "Scheduled-arrival completion/first-event latency includes delay from intended arrival to dispatch. Every scheduled stream is drained; drain time and dispatch lag reveal overload. Fixed-concurrency closed-loop latency only describes that workload. At a fixed offered rate, throughput ratios are achieved completion-rate comparisons, not maximum-capacity ratios."]
    text += ["", "| Protocol | Direction | Event | Bytes | Concurrency | Legacy server CPU ns/event | Schema server CPU ns/event | Legacy client CPU ns/event | Schema client CPU ns/event |",
             "|---|---|---|---:|---:|---:|---:|---:|---:|"]
    for r in data:
        text.append(f'| {r["protocol"]} | {r["direction"]} | {r["kind"]} | {r["size"]} | {r["concurrency"]} | {r["legacy"]["server_cpu_ns_per_event_median"]:.0f} | {r["schema"]["server_cpu_ns_per_event_median"]:.0f} | {r["legacy"]["client_cpu_ns_per_event_median"]:.0f} | {r["schema"]["client_cpu_ns_per_event_median"]:.0f} |')
    text += ["", "Separate allocation passes (absolute heap peak, not a subtraction from idle):", "",
             "| Protocol | Direction | Event | Bytes | Concurrency | Pair | Side | Process | Sampled RSS peak MiB | Requested heap peak MiB | Allocations/event | Requested bytes/event |",
             "|---|---|---|---:|---:|---:|---|---|---:|---:|---:|---:|"]
    for r in rows:
        if r["mode"] != "memory":
            continue
        for role in ("server", "client"):
            metrics = r[role]
            text.append(f'| {r["protocol"]} | {r["direction"]} | {r["kind"]} | {r["size"]} | {r["concurrency"]} | {r.get("pair", 0)} | {r["side"]} | {role} | {r["rss"][role + "_sampled_peak_kib"] / 1024:.2f} | {metrics["after"]["requested_peak_heap_bytes"] / 1048576:.2f} | {metrics["allocations_per_event"]:.2f} | {metrics["allocated_bytes_per_event"]:.1f} |')
    text += ["", "Counter and sampling runs, including availability and exit codes, are recorded per case in `result.json`. Counter attachment brackets the measurement barrier, so its interval also includes a small idle/control margin. Sampling stack/hotspot files are retained beside each profile."]
    (dest / "report.md").write_text("\n".join(text) + "\n")


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--cache", type=Path, default=HERE / "artifacts/cache")
    p.add_argument("--output", type=Path)
    p.add_argument("--preset", choices=["smoke", "representative", "full"], default="representative")
    p.add_argument("--server-cpus", default="0")
    p.add_argument("--client-cpus", default="1,2")
    p.add_argument("--server-workers", type=int, default=1)
    p.add_argument("--no-profiles", action="store_true")
    p.add_argument("--seconds", type=float)
    p.add_argument("--pairs", type=int)
    p.add_argument("--concurrency", type=int, nargs="+", help="In-flight stream limits to sweep")
    p.add_argument("--stream-rate", type=float, default=0, help="Scheduled streams/s; 0 selects closed-loop saturation")
    p.add_argument("--duplex-window", type=int, help="Timestamp queue capacity for latency runs; default is events per stream")
    p.add_argument("--memory-pairs", type=int, default=3)
    p.add_argument("--deep-validation", action="store_true", help="Include full payload scans in measured work; preflight always checks them")
    p.add_argument("--events-per-stream", type=int, help="Event count per stream (at least two)")
    a = p.parse_args()
    cache = a.cache.resolve()
    build = validate_build(cache)
    cpus = [int(c) for c in (a.server_cpus + "," + a.client_cpus).split(",")]
    if len(cpus) != len(set(cpus)):
        p.error("Client and server must use different CPUs")
    topo = topology(cpus)
    if not 1 <= a.server_workers <= len(a.server_cpus.split(",")):
        p.error("Server workers must fit the selected CPUs")
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    dest = a.output.resolve() if a.output else HERE / "artifacts/results" / stamp
    dest.mkdir(parents=True, exist_ok=False)
    write_json(dest / "provenance.json", {"build": build, "generation": json.loads((cache / "generation.json").read_text()), "args": vars(a) | {"cache": str(cache), "output": str(dest)}, "topology": topo, "cpuinfo": Path("/proc/cpuinfo").read_text(), "loadavg": Path("/proc/loadavg").read_text(), "uname": list(os.uname())})
    # argparse contains Path objects; saved explicitly above.
    warmup = 0.2 if a.preset == "smoke" else 5
    seconds = a.seconds if a.seconds is not None else (0.5 if a.preset == "smoke" else 15)
    pairs = a.pairs if a.pairs is not None else (1 if a.preset == "smoke" else 10)
    if not math.isfinite(seconds) or seconds <= 0 or pairs <= 0:
        p.error("--seconds and --pairs must be positive")
    count = a.events_per_stream or (8 if a.preset == "smoke" else 1024)
    if a.events_per_stream is not None and not 2 <= a.events_per_stream <= 2147483647:
        p.error("--events-per-stream must be between 2 and 2147483647")
    sizes = [64, 1024, 65536] if a.preset == "full" else [1024]
    concurrency = a.concurrency or ([1] if a.preset == "smoke" else [1, 4, 16])
    if len(set(concurrency)) != len(concurrency) or any(c <= 0 for c in concurrency):
        p.error("--concurrency must contain distinct positive counts")
    if not math.isfinite(a.stream_rate) or a.stream_rate < 0:
        p.error("--stream-rate must be finite and nonnegative")
    if a.memory_pairs <= 0 or (a.duplex_window is not None and a.duplex_window <= 0):
        p.error("--memory-pairs and --duplex-window must be positive")
    rows = []
    index = 0

    def execute(**kwargs):
        nonlocal index
        index += 1
        print(f'[{index}] {kwargs["protocol"]} {kwargs["side"]} {kwargs["direction"]} {kwargs["kind"]} {kwargs["mode"]}', flush=True)
        return case(cache, dest / f"{index:04d}", server_cpus=a.server_cpus, client_cpus=a.client_cpus, workers=a.server_workers, **kwargs)

    try:
        for protocol, side, direction, kind in itertools.product(PROTOCOLS, ("legacy", "schema"), ("upload", "download", "duplex"), ("structured", "binary")):
            execute(protocol=protocol, side=side, direction=direction, kind=kind, size=64, count=4, concurrency=1, warmup=0, seconds=0, mode="cpu", preflight=True)
        for protocol, direction, kind, size, parallel in itertools.product(PROTOCOLS, ("upload", "download", "duplex"), ("structured", "binary"), sizes, concurrency):
            args = dict(protocol=protocol, direction=direction, kind=kind, size=size, count=count, concurrency=parallel, warmup=warmup, seconds=seconds, stream_rate=a.stream_rate, duplex_window=a.duplex_window, deep_validation=a.deep_validation)
            for pair in range(pairs):
                for side in (("legacy", "schema") if pair % 2 == 0 else ("schema", "legacy")):
                    row = execute(**args, side=side, mode="cpu")
                    row["pair"] = pair
                    write_json(dest / f"{index:04d}" / "result.json", row)
                    rows.append(row)
                    write_json(dest / "results.json", rows)
            for pair in range(1 if a.preset == "smoke" else a.memory_pairs):
                for side in (("legacy", "schema") if pair % 2 == 0 else ("schema", "legacy")):
                    row = execute(**args, side=side, mode="memory")
                    row["pair"] = pair
                    rows.append(row)
            for side in ("legacy", "schema"):
                rows.append(execute(**args, side=side, mode="latency"))
            if direction == "duplex" and parallel == concurrency[-1] and size == 1024 and not a.no_profiles:
                for profile, side in itertools.product(("stat", "record"), ("legacy", "schema")):
                    rows.append(execute(**args, side=side, mode="profile", profile=profile))
            write_json(dest / "results.json", rows)
        validate_build(cache)
        generation = json.loads((cache / "generation.json").read_text())
        report(dest, rows, generation.get("comparison", "unspecified"))
    except BaseException as error:
        write_json(dest / "failure.json", {"error": str(error), "cases_completed": index - 1})
        raise
    print(f"Report: {dest / 'report.md'}", flush=True)


if __name__ == "__main__":
    main()
