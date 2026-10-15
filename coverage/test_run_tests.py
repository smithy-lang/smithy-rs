"""Integration checks for the corpus runner's failure and completeness contract."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


class CorpusRunnerTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.command("java", """
import json, os, pathlib, sys
assert os.environ["SMITHY_RS_STRICT_PROTOCOL_TESTS"] == "1"
config = json.loads(pathlib.Path(sys.argv[sys.argv.index("--config") + 1]).read_text())
output = pathlib.Path(sys.argv[sys.argv.index("--output") + 1])
name, projection = next(iter(config["projections"].items()))
if name == "generation_failure":
    print("intentional generation failure")
    sys.exit(2)
for plugin in projection["plugins"]:
    crate = output / name / plugin
    crate.mkdir(parents=True)
    (crate / "Cargo.toml").write_text("[package]\\nname='fixture'\\nversion='0.0.0'\\n")
    source = "" if name == "empty" else "/// Test ID: ARealProtocolAssertion\\n#[test]\\nfn checks_wire_data() {}\\n"
    if name == "suppressed":
        source += "#[should_panic]\\n#[test]\\nfn hidden_difference() {}\\n"
    if name == "unsupported_assertion":
        source += "// Test serialization is not yet supported\\n"
        source += "/* test case disabled for this protocol (not yet supported) */\\n"
    if name == "unsupported_name":
        source += "/// Test ID: CoverageUnsupportedRegionResponse\\n"
        source += '// model identifier: "test.unsupported#Member"\\n'
    (crate / "lib.rs").write_text(source)
""")
        self.command("cargo", """
import pathlib, sys
manifest = pathlib.Path(sys.argv[sys.argv.index("--manifest-path") + 1])
name = manifest.parent.parent.name
if name == "runtime_failure":
    print("test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out")
    sys.exit(101)
print("test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out")
""")
        self.model = self.root / "fixture.smithy"
        self.model.write_text('$version: "2"\nnamespace test\n')
        self.classpath = self.root / "classpath.txt"
        self.classpath.write_text("fixture-classpath")

    def command(self, name, source):
        path = self.bin / name
        path.write_text(f"#!{sys.executable}\n" + source)
        path.chmod(0o755)

    def run_corpus(self, names, subset=None):
        config = self.root / "config.json"
        config.write_text(json.dumps({"version": "1.0", "projections": {
            name: {"imports": [str(self.model)], "plugins": {"rust-server-codegen": {}}}
            for name in names
        }}))
        output = self.root / "results"
        command = [sys.executable, str(Path(__file__).with_name("run_tests.py")),
                   "--config", str(config), "--classpath", str(self.classpath),
                   "--output", str(output)]
        if subset:
            command += ["--projection", subset]
        result = subprocess.run(command, env={**os.environ, "PATH": f"{self.bin}:{os.environ['PATH']}"},
                                capture_output=True, text=True)
        self.assertTrue((output / "results.json").exists(), result.stderr)
        return result, json.loads((output / "results.json").read_text())

    def test_collects_every_failure_and_still_executes_later_projection(self):
        names = ["generation_failure", "runtime_failure", "empty", "suppressed", "good"]
        result, report = self.run_corpus(names)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertEqual([row["projection"] for row in report["results"]], names)
        self.assertEqual(report["passed_projections"], 1)
        self.assertEqual(report["failed_or_unverified_projections"], 4)
        self.assertTrue(report["complete"])
        self.assertEqual(report["results"][-1]["status"], "passed")
        self.assertEqual(report["results"][2]["artifacts"][0]["status"], "no executable protocol assertions")
        self.assertEqual(report["results"][3]["artifacts"][0]["status"], "suppressed assertions")

    def test_complete_passing_corpus_succeeds(self):
        result, report = self.run_corpus(["good", "also_good"])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertTrue(report["complete"])
        self.assertEqual(report["passed_projections"], 2)

    def test_passing_subset_cannot_claim_complete_corpus(self):
        result, report = self.run_corpus(["good", "runtime_failure"], "good")
        self.assertEqual(result.returncode, 1)
        self.assertFalse(report["complete"])
        self.assertEqual(report["scope"], "partial")

    def test_unsupported_identifiers_are_distinct_from_missing_assertions(self):
        result, report = self.run_corpus(["unsupported_assertion", "unsupported_name"])
        self.assertEqual(result.returncode, 1)
        self.assertEqual(report["results"][0]["artifacts"][0]["status"],
                         "unsupported protocol assertions")
        self.assertEqual(len(report["results"][0]["artifacts"][0]["unsupported_test_comments"]), 2)
        self.assertEqual(report["results"][1]["status"], "passed")
        self.assertEqual(report["passed_projections"], 1)


if __name__ == "__main__":
    unittest.main()
