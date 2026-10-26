#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Validation and grouping tests for comparisons of decoded request traces."""
import copy
from pathlib import Path
import tempfile
import unittest

from compare_traces import FIELDS, comparisons, digest, distribution, segments, summarize_requests


def measured():
    requests = [
        {"status": "200", "outcome": "complete",
         "response_ready_ns": "10", "first_frame_ready_ns": "20", "body_complete_ns": str(ns)}
        for ns in (30, 40, 50)
    ]
    result = {
        "streams": 3,
        "server_timing": {
            "requests_started": 3, "responses_ready": 3, "bodies_completed": 3,
            "active": 0, "errors": 0, "aborted": 0, "statuses": {"200": 3},
            **{field.removesuffix("_ns"): distribution([int(r[field]) for r in requests])
               for field in FIELDS},
        },
    }
    return requests, result


def case(run, side, samples=3, **config):
    value = 100 if side == "legacy" else 80
    return {
        "run": run, "side": side, "case": "0001" if side == "legacy" else "0002",
        "requests": samples, "configuration": config,
        "distributions": {field: distribution([value] * samples) for field in FIELDS},
    }


class TraceComparison(unittest.TestCase):
    def test_raw_samples_use_nearest_rank_quantiles(self):
        requests, result = measured()
        actual = summarize_requests(requests, result)["body_complete_ns"]
        self.assertEqual(actual["samples"], 3)
        self.assertEqual(actual["p50_ns"], 40)
        self.assertEqual(actual["p95_ns"], 50)
        self.assertEqual(actual["mean_ns"], 40)

    def test_bad_counts_outcomes_statuses_and_clock_order_fail(self):
        for mutate in (
            lambda rows, _: rows.pop(),
            lambda rows, _: rows[0].update(outcome="aborted"),
            lambda rows, _: rows[0].update(status="500"),
            lambda rows, _: rows[0].update(first_frame_ready_ns="5"),
            lambda rows, _: rows[0].update(body_complete_ns=True),
            lambda _, result: result["server_timing"].update(active=1),
            lambda _, result: result["server_timing"].update(statuses={"400": 3}),
        ):
            requests, result = measured()
            mutate(requests, result)
            with self.assertRaises(ValueError):
                summarize_requests(requests, result)

    def test_histogram_disagreement_is_detected(self):
        requests, result = measured()
        result["server_timing"]["body_complete"]["p50_ns"] = 4000
        with self.assertRaisesRegex(ValueError, "disagree"):
            summarize_requests(requests, result)

    def test_hdr_rounding_does_not_replace_raw_values(self):
        requests, result = measured()
        result["server_timing"]["body_complete"]["p50_ns"] = 41
        self.assertEqual(summarize_requests(requests, result)["body_complete_ns"]["p50_ns"], 40)

    def test_segments_are_checked_and_sorted_numerically(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            artifacts = []
            for index in reversed(range(12)):
                path = root / f"server-trace-schema.{index}.bin"
                path.write_bytes(str(index).encode())
                artifacts.append({"artifact": f"/previous/location/{path.name}",
                                  "bytes": path.stat().st_size, "sha256": digest(path)})
            result = {"dial9": {"artifacts": artifacts}}
            files = segments(root, result)
            self.assertEqual(files[2].name, "server-trace-schema.2.bin")
            files[2].write_bytes(b"x")
            with self.assertRaisesRegex(ValueError, "checksum"):
                segments(root, result)

    def test_missing_segments_are_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            path = root / "server-trace.1.bin"
            path.write_bytes(b"x")
            result = {"dial9": {"artifacts": [{"artifact": str(path), "bytes": 1,
                                              "sha256": digest(path)}]}}
            with self.assertRaisesRegex(ValueError, "Missing or duplicate"):
                segments(root, result)

    def test_compatible_runs_use_case_ratios_instead_of_pooled_requests(self):
        cases = [case("a", "legacy"), case("a", "schema", samples=300),
                 case("b", "legacy"), case("b", "schema")]
        cases[-1]["distributions"] = {field: distribution([120] * 3) for field in FIELDS}
        group = comparisons(cases)[0]
        stats = group["summary"]["body_complete_ns"]["p50_ns"]
        self.assertEqual(stats["case_pairs"], 2)
        self.assertEqual(stats["median_schema_over_legacy"], 1)

    def test_configurations_remain_separate_and_unmatched_variants_fail(self):
        cases = [case("a", "legacy", concurrency=1), case("a", "schema", concurrency=1),
                 case("b", "legacy", concurrency=4), case("b", "schema", concurrency=4)]
        self.assertEqual(len(comparisons(cases)), 2)
        for changed_config in ("events_per_stream", "build_hash", "host", "measurement_seconds"):
            mismatch = copy.deepcopy(cases[:2])
            mismatch[0]["configuration"][changed_config] = "different"
            with self.assertRaisesRegex(ValueError, "Unmatched"):
                comparisons(mismatch)


if __name__ == "__main__":
    unittest.main()
