"""Heap metrics must reach summaries with their absolute and signed meanings."""
import unittest

from run import summarize


def samples():
    rows = []
    for side, factor in (("baseline", 1), ("current", 2)):
        for repeat, median in ((0, 10), (1, 20)):
            rows.append({
                "protocol": "rest_json", "value_bytes": 64, "side": side,
                "kind": "cpu", "repeat": repeat,
                "cpu_ns_per_request": [(median - 2) * factor, median * factor, (median + 2) * factor],
                "wall_ns_per_request": [(median + 1) * factor, (median + 2) * factor, (median + 3) * factor],
                "peak_rss_kib": (100 + 20 * repeat) * factor,
            })
        rows.append({
            "protocol": "rest_json", "value_bytes": 64, "side": side,
            "kind": "memory", "repeat": 0, "peak_rss_kib": 9999,
            "allocations_per_request": 2.0 * factor,
            "allocated_bytes_per_request": 128.0 * factor,
            "baseline_live_heap_bytes": 128 if side == "baseline" else 96,
            "peak_live_heap_bytes": 192 if side == "baseline" else 96,
            "final_live_heap_bytes": 128 if side == "baseline" else 64,
            "net_heap_delta_bytes": 0 if side == "baseline" else -32,
            "peak_heap_growth_bytes": 64 if side == "baseline" else 0,
        })
    return rows


class SummaryTests(unittest.TestCase):
    def test_absolute_heap_and_signed_delta_are_preserved(self):
        summaries = summarize(samples(), ["rest_json"], [64])
        self.assertEqual(len(summaries), 1)
        record = summaries[0]
        self.assertEqual(record["baseline_baseline_live_heap_bytes"], 128)
        self.assertEqual(record["baseline_peak_live_heap_bytes"], 192)
        self.assertEqual(record["baseline_final_live_heap_bytes"], 128)
        self.assertEqual(record["baseline_net_heap_delta_bytes"], 0)
        self.assertEqual(record["baseline_peak_heap_growth_bytes"], 64)
        self.assertEqual(record["current_baseline_live_heap_bytes"], 96)
        self.assertEqual(record["current_peak_live_heap_bytes"], 96)
        self.assertEqual(record["current_final_live_heap_bytes"], 64)
        self.assertEqual(record["current_net_heap_delta_bytes"], -32)
        self.assertEqual(record["current_peak_heap_growth_bytes"], 0)
        self.assertEqual(record["baseline_allocations_per_request"], 2.0)
        self.assertEqual(record["current_allocated_bytes_per_request"], 256.0)

    def test_timing_rss_and_ratios_keep_existing_meanings(self):
        record = summarize(samples(), ["rest_json"], [64])[0]
        self.assertEqual(record["baseline_cpu_ns"], 15)
        self.assertEqual(record["baseline_cpu_min_run_ns"], 10)
        self.assertEqual(record["baseline_cpu_max_run_ns"], 20)
        self.assertEqual(record["current_cpu_ns"], 30)
        self.assertEqual(record["baseline_wall_ns"], 17)
        self.assertEqual(record["baseline_peak_rss_kib"], 110)
        self.assertEqual(record["current_peak_rss_kib"], 220)
        self.assertEqual(record["cpu_current_over_baseline"], 2)
        self.assertEqual(record["allocated_bytes_current_over_baseline"], 2)

    def test_old_ambiguous_metric_names_are_not_emitted(self):
        record = summarize(samples(), ["rest_json"], [64])[0]
        for side in ("baseline", "current"):
            self.assertNotIn(f"{side}_net_live_heap_bytes", record)
            self.assertNotIn(f"{side}_peak_extra_live_heap_bytes", record)


if __name__ == "__main__":
    unittest.main()
