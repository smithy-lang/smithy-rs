#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Accounting tests that do not generate SDKs or open network listeners."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from common import fingerprint, toolchain, validate_generation, write_json
from run import counters, difference, paired_interval, report, runtime_difference, summary, topology
from prepare import prepare


def row(side, pair, rate, mode="cpu"):
    return {
        "protocol": "rest_json", "side": side, "direction": "duplex", "kind": "binary",
        "size": 1024, "concurrency": 1, "pair": pair, "mode": mode, "events_per_second": rate,
        "server": {"cpu_ns_per_event": 100}, "client": {"cpu_ns_per_event": 200},
    }


def runtime_interval():
    def snapshot(busy, count):
        return {"workers": [{"busy_ns": busy, "polls": count, "parks": count}],
                **{name: [{"start_ns": 0, "end_ns": 100, "worker_counts": [count]},
                          {"start_ns": 100, "end_ns": 2**64 - 1, "worker_counts": [count]}]
                   for name in ("poll_time", "schedule_latency")}}
    return {"before": snapshot(4_000_000_000, 10), "after": snapshot(5_000_000_000, 20),
            "elapsed_seconds": 2, "sample_interval_seconds": 0.1,
            "samples": [{"alive_tasks": 7, "global_queue_depth": 3, "blocking_queue_depth": 2,
                         "blocking_threads": 1, "workers": [{"local_queue_depth": 4, "mean_poll_ns_ewma": 50}]}]}


class Measurements(unittest.TestCase):
    def test_runtime_deltas_use_monitor_window_and_preserve_bucket_bounds(self):
        raw = runtime_interval()
        result = runtime_difference(raw)
        self.assertEqual(result["workers"][0]["busy_ns"], 1_000_000_000)
        self.assertEqual(result["worker_busy_percent_mean"], 50)
        self.assertEqual(result["global_queue_depth_sampled_peak"], 3)
        self.assertEqual(result["workers"][0]["local_queue_depth_sampled_peak"], 4)
        self.assertEqual(result["schedule_latency"]["samples"], 20)
        self.assertEqual(result["schedule_latency"]["p50_bounds_ns"], [0, 100])
        self.assertEqual(result["schedule_latency"]["p99_bounds_ns"], [100, 2**64 - 1])
        before = {"user_seconds": 0, "system_seconds": 0}
        after = {**before, "tokio_runtime": raw}
        self.assertEqual(difference(before, after, 10, 8)["tokio_runtime"]["worker_busy_percent_mean"], 50)
        self.assertEqual(after["tokio_runtime"], raw)

    def test_runtime_rejects_changed_workers_buckets_and_regressed_counters(self):
        for mutate in (
            lambda r: r["after"]["workers"].append({}),
            lambda r: r["after"]["workers"][0].update(busy_ns=0),
            lambda r: r["after"]["poll_time"][0].update(start_ns=1),
            lambda r: r["after"]["schedule_latency"][0].update(worker_counts=[0]),
        ):
            raw = runtime_interval()
            mutate(raw)
            with self.assertRaises(ValueError):
                runtime_difference(raw)

    def test_runtime_report_identifies_scheduler_delay_and_process(self):
        diagnostic = row("schema", 0, 1, "runtime")
        for role in ("server", "client"):
            diagnostic[role]["tokio_runtime"] = runtime_difference(runtime_interval())
        with tempfile.TemporaryDirectory() as tmp:
            report(Path(tmp), [row("legacy", 0, 10), row("schema", 0, 20), diagnostic])
            text = (Path(tmp) / "report.md").read_text()
            self.assertIn("Schedule delay p99 bucket", text)
            self.assertIn("schema | client | 50.00 | 3 | 4 | 7", text)
            self.assertIn("is not CPU time", text)

    def test_cpu_and_allocation_interval_accounting(self):
        before = {"user_seconds": 3, "system_seconds": 1, "allocations": 200, "allocated_bytes": 3000}
        after = {"user_seconds": 5, "system_seconds": 2, "allocations": 240, "allocated_bytes": 4000}
        result = difference(before, after, 20, 2)
        self.assertEqual(result["cpu_seconds"], 3)
        self.assertEqual(result["cpu_percent_of_one_core"], 150)
        self.assertEqual(result["cpu_ns_per_event"], 150_000_000)
        self.assertEqual(result["allocations_per_event"], 2)
        self.assertEqual(result["allocated_bytes_per_event"], 50)

    def test_ratios_are_paired_and_instrumented_runs_excluded(self):
        rows = [row("legacy", 0, 10), row("schema", 1, 10), row("schema", 0, 20), row("legacy", 1, 20), row("schema", 2, 999999, "memory"), row("schema", 2, 999999, "latency"), row("schema", 2, 999999, "runtime")]
        result = summary(rows)[0]
        self.assertEqual(result["paired_throughput_ratio"], {"median": 1.25, "min": 0.5, "max": 2, "pairs": 2})
        self.assertEqual(result["schema"]["events_per_second_median"], 15)

    def test_uncertainty_is_paired_reproducible_and_withheld_for_small_samples(self):
        self.assertIsNone(paired_interval([1, 2, 3])["bootstrap_95_interval"])
        exact = paired_interval([2] * 10)
        self.assertEqual(exact["bootstrap_95_interval"], [2, 2])
        ratios = [0.8, 0.9, 1, 1.1, 1.2] * 2
        result = paired_interval(ratios)
        self.assertEqual(result, paired_interval(ratios))
        low, high = result["bootstrap_95_interval"]
        self.assertLess(low, result["geometric_mean"])
        self.assertGreater(high, result["geometric_mean"])

    def test_same_revision_rejects_a_different_baseline_before_generation(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            with self.assertRaisesRegex(SystemExit, "both servers from --current"):
                prepare(root / "cache", root / "baseline", root / "current", "unused")
            self.assertFalse((root / "cache").exists())

    def test_cached_comparison_design_cannot_be_reused_for_another_design(self):
        with tempfile.TemporaryDirectory() as tmp:
            cache = Path(tmp)
            write_json(cache / "generation.json", {})
            with patch("prepare.validate_generation", return_value={"comparison": "release"}):
                with self.assertRaisesRegex(SystemExit, "different comparison design"):
                    prepare(cache, cache, cache, "unused")

    def test_unpaired_runs_fail(self):
        with self.assertRaises(AssertionError):
            summary([row("legacy", 0, 10), row("schema", 1, 20)])
        with self.assertRaises(AssertionError):
            summary([row("legacy", 0, 10), row("schema", 0, 20), row("legacy", 1, 10)])

    def test_unavailable_counters_are_not_zero(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "perf.csv"
            path.write_text("1200,,cycles:u,100,100.00\n<not supported>,,instructions:u,0,100.00\n")
            parsed = counters(path, 100)
            self.assertEqual(parsed["cycles:u"]["per_event"], 12)
            self.assertEqual(parsed["instructions:u"]["unavailable"], "<not supported>")
            self.assertNotIn("value", parsed["instructions:u"])

    def test_report_retains_range_and_sample_count(self):
        with tempfile.TemporaryDirectory() as tmp:
            report(Path(tmp), [row("legacy", 0, 10), row("schema", 0, 20)])
            self.assertIn("n=1", (Path(tmp) / "report.md").read_text())
            self.assertEqual(json.loads((Path(tmp) / "summary.json").read_text())[0]["paired_throughput_ratio"]["pairs"], 1)

    def test_source_fingerprint_detects_untracked_source_and_ignores_build_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "rust-runtime" / "test" / "src"
            source.mkdir(parents=True)
            (source / "lib.rs").write_text("source one")
            first = fingerprint(root)
            generated = root / "rust-runtime" / "test" / "target"
            generated.mkdir()
            (generated / "output").write_text("ignored")
            self.assertEqual(first, fingerprint(root))
            (source / "new.rs").write_text("new source")
            self.assertNotEqual(first, fingerprint(root))

    def test_stale_generation_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp, patch("common.generation_hash", return_value="current"):
            write_json(Path(tmp) / "generation.json", {"generation_hash": "old"})
            with self.assertRaisesRegex(SystemExit, "Generation inputs changed"):
                validate_generation(Path(tmp))

    def test_duplicate_smt_groups_are_rejected(self):
        with patch.object(Path, "exists", return_value=True), patch.object(Path, "read_text", return_value="0,32"):
            with self.assertRaisesRegex(SystemExit, "share a physical core"):
                topology([0, 32])

    def test_different_toolchain_pins_are_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            paths = {}
            for side, version in (("legacy", "1.94.1"), ("schema", "1.95.0")):
                root = Path(tmp) / side
                root.mkdir()
                (root / "rust-toolchain.toml").write_text(f'[toolchain]\nchannel = "{version}"\n')
                paths[side] = {"repo": str(root)}
            with self.assertRaisesRegex(SystemExit, "different compilers"):
                toolchain(paths)


if __name__ == "__main__":
    unittest.main()
