/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Running one of the canary's workflow steps, for real, against a stand-in `aws`.
//!
//! `tests/workflows.rs` asserts properties of these workflows by reading them. Two kinds of defect
//! survive that and fail only when a step runs:
//!
//! * a shell construct whose exit status ends the step — `grep -v` on an empty list under
//!   `set -o pipefail`, which stops the arming job before `put-metric-alarm`.
//! * a Python snippet reading a key the JSON it parses does not carry, which compiles perfectly
//!   and is a `KeyError` at release time.
//!
//! Both are invisible to a reader of the YAML and obvious the first time the script runs.
//!
//! The step is extracted from the shipped workflow rather than copied, so these cannot drift from
//! what runs, and is executed the way GitHub executes it: `bash --noprofile --norc -eo pipefail`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_yaml::Value;

/// Whether this platform can run the steps at all.
///
/// Returning rather than failing: the hazards these tests cover are the same everywhere, and a
/// developer on a machine without `bash` should not be blocked by a test that cannot speak to it.
/// CI runs on Linux, which is where it matters that this does not silently skip.
pub fn bash_available() -> bool {
    Command::new("bash")
        .args(["-c", "true"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("the repository root is three levels above this crate")
        .to_path_buf()
}

/// One step's `run:` script, by workflow, job and exact step name.
///
/// The name is matched exactly. A case-insensitive or partial match reads more forgivingly and is
/// a trap: lowercased, `arm` is inside `is the alarm configured`, so the obvious spelling selects
/// the wrong step.
pub fn step_script(workflow: &str, job: &str, step: &str) -> String {
    let path = repo_root()
        .join(".github/workflows")
        .join(format!("{workflow}.yml"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let parsed: Value =
        serde_yaml::from_str(&text).unwrap_or_else(|e| panic!("parsing {}: {e}", path.display()));

    let steps = parsed
        .get("jobs")
        .and_then(|j| j.get(job))
        .and_then(|j| j.get("steps"))
        .and_then(Value::as_sequence)
        .unwrap_or_else(|| panic!("no steps for job {job} in {workflow}.yml"));

    let matching: Vec<&Value> = steps
        .iter()
        .filter(|s| s.get("name").and_then(Value::as_str) == Some(step))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one step named {step:?} in {workflow}.yml {job}, found {}. Steps: {}",
        matching.len(),
        steps
            .iter()
            .map(|s| s
                .get("name")
                .or_else(|| s.get("uses"))
                .and_then(Value::as_str)
                .unwrap_or("?")
                .to_string())
            .collect::<Vec<_>>()
            .join(" | ")
    );

    matching[0]
        .get("run")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("step {step:?} has no `run:` script"))
        .to_string()
}

/// A directory that removes itself, so a failing test leaves nothing behind.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "canary-step-{}-{}-{}",
            label,
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(dir.join("bin")).expect("creating a scratch directory");
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What a step did.
pub struct Outcome {
    /// `None` when the step was killed by a signal rather than exiting.
    pub status: Option<i32>,
    /// Everything the step wrote to stdout and stderr, which on a runner is the public job log.
    pub log: String,
    /// What reached `$GITHUB_STEP_SUMMARY`, which on a public repository is also public.
    pub summary: String,
    /// What reached `$GITHUB_OUTPUT`.
    pub output: String,
    /// Every command line the stand-in `aws` was called with, newest last.
    ///
    /// Without this a step that short-circuited before reaching the API could satisfy every
    /// assertion about what it did *not* publish, having published nothing because it did nothing.
    pub calls: String,
}

impl Outcome {
    pub fn succeeded(&self) -> bool {
        self.status == Some(0)
    }

    /// Assert something does not appear in the log, refusing to do so vacuously.
    ///
    /// An empty log is not evidence of absence: a step that died before producing output satisfies
    /// every absence assertion trivially, reporting a pass for a step that never ran a command.
    pub fn log_lacks(&self, needle: &str) {
        assert!(
            !self.log.is_empty(),
            "nothing to search: the step produced no output, so {needle:?} is not absent from the \
             log, it is unobserved"
        );
        assert!(
            !self.log.contains(needle),
            "{needle:?} reached the job log:\n{}",
            self.log
        );
    }

    /// The same, for the job summary.
    pub fn summary_lacks(&self, needle: &str) {
        assert!(
            !self.summary.is_empty(),
            "nothing to search: the step wrote no summary, so {needle:?} is not absent from it"
        );
        assert!(
            !self.summary.contains(needle),
            "{needle:?} reached the job summary:\n{}",
            self.summary
        );
    }
}

/// A step, its environment and its stand-in `aws`, ready to run.
pub struct StepRunner {
    script: String,
    label: String,
    env: BTreeMap<String, String>,
    stub: Option<String>,
}

impl StepRunner {
    pub fn new(workflow: &str, job: &str, step: &str) -> Self {
        StepRunner {
            script: step_script(workflow, job, step),
            label: format!("{workflow}-{job}"),
            env: BTreeMap::new(),
            stub: None,
        }
    }

    /// The step's source, for reading a value the step pins rather than assuming it.
    pub fn script(&self) -> &str {
        &self.script
    }

    pub fn env(mut self, key: &str, value: impl Into<String>) -> Self {
        self.env.insert(key.to_string(), value.into());
        self
    }

    /// The body of a stand-in `aws`, placed first on `PATH`.
    ///
    /// It is written by the test rather than checked in, so there is no fake CLI in the repository
    /// to drift from the real one's interface.
    pub fn stub_aws(mut self, body: &str) -> Self {
        self.stub = Some(body.to_string());
        self
    }

    pub fn run(self) -> Outcome {
        let scratch = Scratch::new(&self.label);
        let dir = &scratch.0;

        let script = dir.join("step.sh");
        std::fs::write(&script, format!("#!/usr/bin/env bash\n{}", self.script))
            .expect("writing the step");

        let calls = dir.join("calls");
        let summary = dir.join("summary");
        let output = dir.join("output");
        for f in [&calls, &summary, &output] {
            std::fs::write(f, "").expect("creating a capture file");
        }

        let mut command = Command::new("bash");
        command
            .args(["--noprofile", "--norc", "-eo", "pipefail"])
            .arg(&script)
            // GitHub's own, which several steps append to.
            .env("GITHUB_STEP_SUMMARY", &summary)
            .env("GITHUB_OUTPUT", &output)
            .env("RUNNER_TEMP", dir)
            .env("CALLS", &calls)
            .current_dir(dir);

        if let Some(body) = &self.stub {
            let aws = dir.join("bin/aws");
            std::fs::write(&aws, format!("#!/usr/bin/env bash\n{body}"))
                .expect("writing the stand-in aws");
            make_executable(&aws);
            let path = std::env::var("PATH").unwrap_or_default();
            command.env("PATH", format!("{}:{path}", dir.join("bin").display()));
        }

        for (key, value) in &self.env {
            command.env(key, value);
        }

        let done = command.output().expect("running the step");
        let read = |p: &Path| std::fs::read_to_string(p).unwrap_or_default();
        Outcome {
            status: done.status.code(),
            log: format!(
                "{}{}",
                String::from_utf8_lossy(&done.stdout),
                String::from_utf8_lossy(&done.stderr)
            ),
            summary: read(&summary),
            output: read(&output),
            calls: read(&calls),
        }
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path).expect("stat").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).expect("chmod");
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}

/// The value a step pins for `name`, read out of the step rather than assumed.
///
/// `SEVERITY` is the one that matters: it changes as the rollout proceeds, and a test asserting a
/// literal would start passing for the wrong reason the day it moved.
pub fn pinned(script: &str, name: &str) -> String {
    let prefix = format!("{name}=");
    let found: Vec<&str> = script
        .lines()
        .filter_map(|l| l.trim().strip_prefix(&prefix))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "expected exactly one {prefix} line in the step, found {found:?}"
    );
    found[0].to_string()
}
