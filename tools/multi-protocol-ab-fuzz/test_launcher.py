# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Exercise launcher ordering and failure handling without building or running AFL."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


class LauncherTests(unittest.TestCase):
    def run_launcher(self, prepared=False, generation_status=0, init_status=0,
                     replay_status=0, dirty=False):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            scripts = root / "scripts"
            # env.sh puts the campaign's driver directory first on PATH.
            commands = root / "output/driver/bin"
            repo = root / "repo"
            for directory in (scripts, commands, repo):
                directory.mkdir(parents=True)
            source = Path(__file__).resolve().parent
            for name in ("fuzz.sh", "env.sh"):
                shutil.copyfile(source / name, scripts / name)

            def executable(path, body):
                path.write_text("#!/usr/bin/env bash\n" + body + "\n")
                path.chmod(0o755)

            executable(repo / "gradlew", "exit 0")
            (repo / "rust-toolchain.toml").write_text('channel = "stable"\n')
            executable(commands / "git",
                       'case "$*" in *status*) '
                       + ('echo modified' if dirty else 'true')
                       + ' ;; *) echo "$CURRENT_REPO" ;; esac')
            for name in ("java", "cargo", "rustup", "sleep"):
                executable(commands / name, "exit 0")
            executable(commands / "aws-smithy-fuzz", "exit 0")
            executable(commands / "timeout",
                       'echo fuzz >> "$LAUNCH_EVENTS"\nexit 124')
            executable(scripts / "generate.sh",
                       f'echo generate >> "$LAUNCH_EVENTS"\nexit {generation_status}')
            executable(scripts / "init.sh",
                       f'echo init >> "$LAUNCH_EVENTS"\nexit {init_status}')
            executable(scripts / "replay.sh",
                       f'echo replay >> "$LAUNCH_EVENTS"\nexit {replay_status}')
            work = root / "output/work-isolated-v1/pokemon/rest-json1"
            work.mkdir(parents=True)
            (work / "smithy-fuzz-config.json").write_text("{}")
            saved = work / "afl-output/crashes/saved"
            saved.parent.mkdir(parents=True)
            saved.write_text("existing finding")
            events = root / "events"
            env = dict(os.environ)
            env.update(BASELINE_REPO=str(repo), CURRENT_REPO=str(repo),
                       FUZZ_ROOT=str(root / "output"), SUITES="pokemon",
                       CASES="rest-json1", LAUNCH_EVENTS=str(events),
                       PATH=str(commands) + os.pathsep + env["PATH"])
            args = ["bash", str(scripts / "fuzz.sh")]
            if prepared:
                args.append("--prepared")
            result = subprocess.run(args + ["1", "1"], env=env,
                                    capture_output=True, text=True)
            self.assertEqual(saved.read_text(), "existing finding")
            return result, events.read_text().splitlines() if events.exists() else []

    def test_default_runs_every_stage_and_preserves_findings(self):
        result, events = self.run_launcher()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(events, ["generate", "init", "fuzz", "replay"])

    def test_generation_failure_stops_later_stages(self):
        result, events = self.run_launcher(generation_status=3)
        self.assertEqual(result.returncode, 3)
        self.assertEqual(events, ["generate"])

    def test_seed_failure_stops_before_fuzzing(self):
        result, events = self.run_launcher(init_status=4)
        self.assertEqual(result.returncode, 4)
        self.assertEqual(events, ["generate", "init"])

    def test_prepared_skips_setup_and_reports_replay_failure(self):
        result, events = self.run_launcher(prepared=True, replay_status=5)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(events, ["fuzz", "replay"])

    def test_dirty_baseline_stops_before_generation(self):
        result, events = self.run_launcher(dirty=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Baseline must be clean", result.stderr)
        self.assertEqual(events, [])

    def test_help_does_not_require_a_baseline(self):
        env = dict(os.environ)
        env.pop("BASELINE_REPO", None)
        result = subprocess.run(
            ["bash", str(Path(__file__).with_name("fuzz.sh")), "--help"],
            env=env, capture_output=True, text=True,
        )
        self.assertEqual(result.returncode, 0)
        self.assertIn("--prepared", result.stdout)


if __name__ == "__main__":
    unittest.main()
