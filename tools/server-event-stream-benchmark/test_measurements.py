#!/usr/bin/env python3
"""Accounting tests that do not generate SDKs or open network listeners."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from common import fingerprint, toolchain, validate_generation, write_json
from run import counters, difference, report, summary, topology


def row(side, pair, rate, mode="cpu"):
    return {
        "protocol": "rest_json", "side": side, "direction": "duplex", "kind": "binary",
        "size": 1024, "concurrency": 1, "pair": pair, "mode": mode, "events_per_second": rate,
        "server": {"cpu_ns_per_event": 100}, "client": {"cpu_ns_per_event": 200},
    }


class Measurements(unittest.TestCase):
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
        rows = [row("legacy", 0, 10), row("schema", 1, 10), row("schema", 0, 20), row("legacy", 1, 20), row("schema", 2, 999999, "memory")]
        result = summary(rows)[0]
        self.assertEqual(result["paired_throughput_ratio"], {"median": 1.25, "min": 0.5, "max": 2, "pairs": 2})
        self.assertEqual(result["schema"]["events_per_second_median"], 15)

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
