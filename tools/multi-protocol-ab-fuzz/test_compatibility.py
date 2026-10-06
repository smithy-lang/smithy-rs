"""Shared acceptance contract for the Rust fuzz driver and Python triage."""
import json
from pathlib import Path
import unittest
from unittest.mock import patch
import abfuzz


class CompatibilityTests(unittest.TestCase):
    def test_known_divergence_contract(self):
        root = Path(__file__).resolve().parents[2]
        cases = json.loads((root / "rust-runtime/aws-smithy-fuzz/tests/known-divergences.json").read_text())
        with patch.object(abfuzz, "SEMANTIC", True), patch.object(abfuzz, "IGNORE_UNROUTED", False):
            for case in cases:
                with self.subTest(case=case["name"]):
                    r = case["request"]
                    request = (r["uri"], r["method"], r["headers"], bytes(r["body"]))
                    def convert(value):
                        return {**value["response"], "body": bytes(value["response"]["body"]), "input": value["input"]}
                    self.assertEqual(abfuzz.known_divergence(request, convert(case["baseline"]),
                        convert(case["candidate"])) is not None, case["known"])

    def test_xml_claim_divergence_contract(self):
        root = Path(__file__).resolve().parents[2]
        cases = json.loads((root / "rust-runtime/aws-smithy-fuzz/tests/xml-claim-divergences.json").read_text())
        with patch.object(abfuzz, "SEMANTIC", True), patch.object(abfuzz, "IGNORE_UNROUTED", False):
            for case in cases:
                with self.subTest(case=case["name"]):
                    r = case["request"]
                    request = (r["uri"], r["method"], r["headers"], bytes(r["body"]))
                    def convert(value):
                        return {**value["response"], "body": bytes(value["response"]["body"]), "input": value["input"]}
                    self.assertEqual(abfuzz.known_divergence(request, convert(case["baseline"]),
                        convert(case["candidate"])) is not None, case["known"])

    def test_xml_event_error_divergence_contract(self):
        root = Path(__file__).resolve().parents[2]
        cases = json.loads((root / "rust-runtime/aws-smithy-fuzz/tests/xml-event-error-divergences.json").read_text())
        with patch.object(abfuzz, "SEMANTIC", True), patch.object(abfuzz, "IGNORE_UNROUTED", False):
            for case in cases:
                with self.subTest(case=case["name"]):
                    r = case["request"]
                    request = (r["uri"], r["method"], r["headers"], bytes(r["body"]))
                    def convert(value):
                        return {**value["response"], "body": bytes(value["response"]["body"]), "input": value["input"]}
                    self.assertEqual(abfuzz.known_divergence(request, convert(case["baseline"]),
                        convert(case["candidate"])) is not None, case["known"])

    def test_shared_contract(self):
        root = Path(__file__).resolve().parents[2]
        cases = json.loads((root / "rust-runtime/aws-smithy-fuzz/tests/compatibility.json").read_text())
        with patch.object(abfuzz, "SEMANTIC", True), patch.object(abfuzz, "IGNORE_UNROUTED", False):
            for case in cases:
                with self.subTest(case=case["name"]):
                    def convert(value):
                        return {**value["response"], "body": bytes(value["response"]["body"]), "input": value["input"]}
                    self.assertEqual(abfuzz.describe(convert(case["b"]), convert(case["a"])) is None, case["agree"])
