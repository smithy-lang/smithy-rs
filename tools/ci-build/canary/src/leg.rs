/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Running one leg: six commands, retries, and a deadline that always reports.
//!
//! # The retry policy is chosen by the verdict, not by the step
//!
//! This is what lets the table below be expressed directly rather than approximated. The same command
//! — `cargo generate-lockfile` — gets six minutes of patience when the registry is unreachable and
//! one minute when cargo has *refused* to resolve, because those are different failures that happen
//! to share a step.
//!
//! | Verdict of the attempt | Attempts | Spacing | Why |
//! |---|---|---|---|
//! | `INFRA_FAIL` | 3 | 3 min | a blip must persist 6 minutes to page |
//! | `RESOLVE_FAIL` | 2 | 1 min | waits for crates.io propagation, not for a blip |
//! | any build or test verdict | 2 | immediate | `--offline` and deterministic; a retry only catches a flaky runner |
//!
//! # Exhaustion ends the leg
//!
//! A step that exhausts its attempts emits and exits, so no later step runs. That is what keeps
//! per-step retries from compounding: the only way to reach step *n+1* is for step *n* to have
//! succeeded, and a step that succeeded owes the next one nothing.
//!
//! # The deadline reports rather than dies
//!
//! `timeout-minutes: 14` on the job is a backstop that should never fire, because a job killed by
//! GitHub runs no further steps and so emits nothing — the silent failure this project exists to
//! remove. Instead the leg tracks its own clock and, when too little remains for another attempt,
//! stops retrying and emits `INFRA_FAIL` itself.

use std::fmt;
use std::time::Duration;

use serde::Serialize;

use crate::classify::{self, Diagnosis, Step, StepOutcome, Verdict};
use crate::scenarios::{Body, Checks, Leg, Profile};

/// The leg's own deadline. Strictly under the watcher's 15-minute dispatch tick, so a leg can never
/// outlive the tick that dispatched it and be re-dispatched alongside itself.
pub const BUDGET: Duration = Duration::from_secs(14 * 60);

/// Attempts and spacing for one verdict class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retry {
    pub attempts: u32,
    pub interval: Duration,
}

impl Retry {
    /// The policy table in this module's documentation.
    pub fn for_verdict(v: Verdict) -> Self {
        match v {
            Verdict::InfraFail => Retry {
                attempts: 3,
                interval: Duration::from_secs(3 * 60),
            },
            Verdict::ResolveFail => Retry {
                attempts: 2,
                interval: Duration::from_secs(60),
            },
            Verdict::CompileFail | Verdict::HarnessFail | Verdict::RuntimeFail => Retry {
                attempts: 2,
                interval: Duration::ZERO,
            },
        }
    }
}

/// A command the leg runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Command {
    pub step: Step,
    pub program: String,
    pub args: Vec<String>,
    /// Whether to ask cargo for `--message-format=json`, which the classifier needs for the
    /// compile steps and must *not* have for the run step, where the evidence is the harness's text.
    pub json: bool,
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.program, self.args.join(" "))
    }
}

/// The six commands for a leg, in order, already carrying its toolchain, target and profile.
pub fn commands(leg: &Leg) -> Vec<Command> {
    let tc = format!("+{}", leg.toolchain_version);
    let target: Vec<String> = match &leg.target {
        Some(t) => vec!["--target".into(), t.clone()],
        None => vec![],
    };
    let release: Vec<String> = match leg.profile {
        Profile::Release => vec!["--release".into()],
        Profile::Debug => vec![],
    };

    let cargo = |step: Step, sub: &[&str], json: bool| {
        let mut args = vec![tc.clone()];
        args.extend(sub.iter().map(|s| s.to_string()));
        args.extend(target.clone());
        args.extend(release.clone());
        if json {
            args.push("--message-format=json".into());
        }
        Command {
            step,
            program: "cargo".into(),
            args,
            json,
        }
    };

    // The toolchain comes first because every later command is invoked as `cargo +<version>`.
    // Recent rustup auto-installs a missing `+toolchain` and only warns, which is why running
    // `generate-lockfile` first appeared to work locally; it makes the leg depend on an opt-out-able
    // rustup convenience (`RUSTUP_AUTO_INSTALL=0` disables it) for a step that is free to order
    // correctly. Installing explicitly also keeps the download inside a step whose failures carry
    // the `INFRA_FAIL` retry policy.
    let mut out = vec![Command {
        step: Step::InstallToolchain,
        program: "rustup".into(),
        args: vec![
            "toolchain".into(),
            "install".into(),
            "--profile".into(),
            "minimal".into(),
            leg.toolchain_version.clone(),
        ],
        json: false,
    }];

    // Installing a toolchain does not bring its cross targets. Without this the build fails with
    // `E0463: can't find crate for core` — a compiler diagnostic that looks exactly like a broken
    // graph but is a mis-set-up runner. CI's `Canary (aarch64)` job does the same thing with
    // `rustup target add aarch64-unknown-linux-musl --toolchain 1.94.1`.
    if let Some(t) = &leg.target {
        out.push(Command {
            step: Step::InstallToolchain,
            program: "rustup".into(),
            args: vec![
                "target".into(),
                "add".into(),
                t.clone(),
                "--toolchain".into(),
                leg.toolchain_version.clone(),
            ],
            json: false,
        });
    }

    // No --offline: this is the step that reaches the registry on purpose, and the one that owns
    // RESOLVE_FAIL. Everything after it runs against the lockfile it just wrote.
    out.push(Command {
        step: Step::GenerateLockfile,
        program: "cargo".into(),
        args: vec![tc.clone(), "generate-lockfile".into()],
        json: false,
    });
    out.push(Command {
        step: Step::Fetch,
        program: "cargo".into(),
        args: vec![tc.clone(), "fetch".into(), "--locked".into()],
        json: false,
    });
    out.push(cargo(Step::Build, &["build", "--offline"], true));

    // A cross-compiled leg builds but cannot execute, so it stops here. Asking it to run tests
    // would need the architecture emulation CI calls "a more complicated setup".
    if leg.checks == Checks::Full {
        out.push(cargo(
            Step::TestNoRun,
            &["test", "--no-run", "--offline"],
            true,
        ));
        // No JSON: by this point the compile is a no-op and the evidence is the harness's output.
        out.push(cargo(Step::Test, &["test", "--offline"], false));
    }
    out
}

/// Executes a command. Injected so the retry and deadline logic is testable without sleeping.
pub trait Exec {
    /// Run one command, giving up after `remaining`.
    ///
    /// The bound is the caller's remaining budget rather than a per-step constant, so there is no
    /// second number to keep in step with the leg's 14-minute limit. An implementation that exceeds
    /// it hands the kill decision to GitHub, which reports nothing.
    fn run(&mut self, command: &Command, remaining: Duration) -> StepOutcome;
}

/// Elapsed time and sleeping. Injected for the same reason.
pub trait Clock {
    fn elapsed(&self) -> Duration;
    fn sleep(&mut self, d: Duration);
}

/// Runs commands for real, in `dir`.
pub struct RealExec {
    pub dir: std::path::PathBuf,
    /// Echo each command and its output, so a red leg is diagnosable from the job log alone.
    pub verbose: bool,
}

/// How often the bounded wait checks whether the child has finished. Short enough that a leg does
/// not idle noticeably past its deadline, long enough not to spin.
const POLL: Duration = Duration::from_millis(200);

/// Read one of the child's pipes to EOF on its own thread.
///
/// Both pipes must be drained concurrently with the wait. Polling `try_wait` while the child writes
/// to a full pipe deadlocks: the child blocks on the write, so it never exits, so the poll never
/// ends. `cargo build --message-format=json` emits megabytes — far past any pipe buffer — so this is
/// the ordinary case and not a corner one.
fn drain<R: std::io::Read + Send + 'static>(src: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut src) = src {
            let _ = src.read_to_end(&mut buf);
        }
        buf
    })
}

impl Exec for RealExec {
    fn run(&mut self, command: &Command, remaining: Duration) -> StepOutcome {
        if self.verbose {
            eprintln!("+ {command}");
        }
        let outcome = |exit_code, stdout: String, stderr: String, timed_out| StepOutcome {
            step: command.step,
            exit_code,
            stdout,
            stderr,
            // The runner sets this; the executor does not know the leg.
            expects_assertions: false,
            timed_out,
        };

        let spawned = std::process::Command::new(&command.program)
            .args(&command.args)
            .current_dir(&self.dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn();

        let mut child = match spawned {
            Ok(c) => c,
            // The program could not be spawned at all — rustup or cargo missing from PATH. That is
            // the runner, so it must look like infra rather than be allowed to panic.
            Err(e) => {
                return outcome(
                    None,
                    String::new(),
                    format!("failed to spawn {}: {e}", command.program),
                    false,
                )
            }
        };

        let out_reader = drain(child.stdout.take());
        let err_reader = drain(child.stderr.take());

        // The bound that makes a hang reportable. Without it this was `output()`, which blocks
        // until the child exits: a stalled network read in `generate-lockfile` or a toolchain
        // download meant the leg never returned, GitHub killed the job at `timeout-minutes`, and a
        // killed job runs no further steps and so emits *nothing*. Nothing is worse than a red
        // verdict, because an absent datapoint cannot raise an alarm.
        let started = std::time::Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Err(e) => {
                    return outcome(None, String::new(), format!("could not wait: {e}"), false)
                }
                Ok(None) => {}
            }
            if started.elapsed() >= remaining {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            std::thread::sleep(POLL);
        };

        // Joining after the child is gone: both pipes are closed, so the readers finish promptly
        // whether it exited or was killed, and whatever it managed to write is still captured.
        let stdout = String::from_utf8_lossy(&out_reader.join().unwrap_or_default()).into_owned();
        let stderr = String::from_utf8_lossy(&err_reader.join().unwrap_or_default()).into_owned();

        if self.verbose && !stderr.trim().is_empty() {
            eprintln!("{stderr}");
        }
        // Echo stdout too, but not the json: cargo's `--message-format=json` stdout is machine
        // output and megabytes of it would bury the log. The run phase's stdout is the test
        // harness's own report — which assertions ran, and how many passed — and omitting it meant a
        // green leg's log could not be told from one that ran nothing.
        if self.verbose && !command.json && !stdout.trim().is_empty() {
            eprintln!("{stdout}");
        }

        match status {
            Some(status) => outcome(status.code(), stdout, stderr, false),
            None => {
                if self.verbose {
                    eprintln!(
                        "! killed after {:?}: the leg's remaining budget was {remaining:?}",
                        started.elapsed()
                    );
                }
                let note = format!(
                    "the canary killed this command after {:?}, which was all of the leg's \
                     remaining budget",
                    started.elapsed()
                );
                outcome(None, stdout, format!("{stderr}\n{note}"), true)
            }
        }
    }
}

/// Wall-clock time, sleeping for real.
pub struct RealClock {
    start: std::time::Instant,
}

impl Default for RealClock {
    fn default() -> Self {
        Self {
            start: std::time::Instant::now(),
        }
    }
}

impl Clock for RealClock {
    fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
    fn sleep(&mut self, d: Duration) {
        eprintln!("… retrying in {}s", d.as_secs());
        std::thread::sleep(d);
    }
}

/// One attempt, for the log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Attempt {
    pub step: Step,
    pub command: String,
    pub attempt: u32,
    pub succeeded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
    pub at: Duration,
}

/// Why the leg stopped retrying.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stopped {
    /// The step's attempts ran out.
    AttemptsExhausted,
    /// Too little of the budget remained for another attempt. The leg reports rather than letting
    /// GitHub kill it silently.
    DeadlineReached,
}

/// What the leg concluded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub leg_id: String,
    /// `None` means green.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_step: Option<Step>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnosis: Option<Diagnosis>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped: Option<Stopped>,
    pub attempts: Vec<Attempt>,
    pub elapsed: Duration,
}

impl Report {
    pub fn is_green(&self) -> bool {
        self.verdict.is_none()
    }

    /// The process exit code. A floor hour's exit code carries the stop decision, because `needs:`
    /// can only see success or failure and never *why*: green and `INFRA_FAIL` exit 0 so the window
    /// continues, and every hard verdict exits non-zero so the remaining hours are skipped.
    pub fn exit_code(&self) -> i32 {
        match self.verdict {
            None => 0,
            Some(v) if v.stops_the_window() => 1,
            Some(_) => 0,
        }
    }
}

/// Run a leg to completion.
pub fn run(leg: &Leg, exec: &mut dyn Exec, clock: &mut dyn Clock, budget: Duration) -> Report {
    let mut attempts = Vec::new();

    for command in commands(leg) {
        let mut attempt = 0u32;
        // Re-derived each time round: the policy depends on *this* failure's verdict, so a step
        // whose failure mode changes between attempts is governed by the latest one.
        loop {
            attempt += 1;
            // Each command may use whatever is left of the leg's budget and no more. Deriving it
            // rather than configuring a per-step timeout means there is no second number to keep
            // consistent with the 14-minute bound, and the leg cannot overrun however it fails.
            let remaining = budget.saturating_sub(clock.elapsed());
            let mut outcome = exec.run(&command, remaining);
            // Whether this leg has assertions is a property of the leg, not of the command, so the
            // executor stays ignorant of it and the runner fills it in. Only the app body renders
            // `tests/hermetic.rs`; a stub body running zero tests is correct, not suspicious.
            outcome.expects_assertions = leg.body == Body::CanaryApp;
            let classified = classify::classify(&outcome);
            attempts.push(Attempt {
                step: command.step,
                command: command.to_string(),
                attempt,
                succeeded: classified.is_none(),
                verdict: classified.as_ref().map(|(v, _)| *v),
                at: clock.elapsed(),
            });

            let Some((verdict, diagnosis)) = classified else {
                break; // this command passed; go to the next
            };

            let policy = Retry::for_verdict(verdict);
            let exhausted = attempt >= policy.attempts;
            // Only sleep if there is room for the wait itself. Checking before sleeping rather than
            // after is what keeps the leg inside its budget.
            let no_room = clock.elapsed() + policy.interval >= budget;

            if exhausted || no_room {
                return Report {
                    leg_id: leg.id.clone(),
                    verdict: Some(verdict),
                    failed_step: Some(command.step),
                    diagnosis: Some(diagnosis),
                    stopped: Some(if exhausted {
                        Stopped::AttemptsExhausted
                    } else {
                        Stopped::DeadlineReached
                    }),
                    attempts,
                    elapsed: clock.elapsed(),
                };
            }
            clock.sleep(policy.interval);
        }
    }

    Report {
        leg_id: leg.id.clone(),
        verdict: None,
        failed_step: None,
        diagnosis: None,
        stopped: None,
        attempts,
        elapsed: clock.elapsed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenarios;
    use crate::toolchain::{self, Version};

    fn leg_named(id: &str) -> Leg {
        let tcs = toolchain::resolve(Version::new(1, 99, 1), Version::new(1, 94, 1))
            .unwrap()
            .kept;
        scenarios::legs(&tcs)
            .into_iter()
            .find(|l| l.id == id)
            .unwrap()
    }

    /// Advances only when the code under test sleeps, so a six-minute retry sequence runs instantly
    /// and the test asserts on the *durations* rather than enduring them.
    #[derive(Default)]
    struct FakeClock {
        now: Duration,
        slept: Vec<Duration>,
    }
    impl Clock for FakeClock {
        fn elapsed(&self) -> Duration {
            self.now
        }
        fn sleep(&mut self, d: Duration) {
            self.slept.push(d);
            self.now += d;
        }
    }

    /// Returns scripted outcomes keyed by step, and counts calls.
    struct FakeExec {
        /// For each step, the sequence of exit codes to return. Exhausting it yields success.
        script: Vec<(Step, Vec<i32>)>,
        stdout: String,
        stderr: String,
        calls: Vec<Step>,
        /// A step that hangs: the real executor would kill it for exceeding the remaining budget.
        hangs: Option<Step>,
        /// The `remaining` each call was given, so the test can check the budget is being handed
        /// down rather than silently ignored.
        budgets: Vec<Duration>,
    }
    impl FakeExec {
        fn always_ok() -> Self {
            Self {
                script: vec![],
                stdout: String::new(),
                stderr: String::new(),
                calls: vec![],
                hangs: None,
                budgets: vec![],
            }
        }
        fn failing(step: Step, codes: Vec<i32>, stdout: &str, stderr: &str) -> Self {
            Self {
                script: vec![(step, codes)],
                stdout: stdout.into(),
                stderr: stderr.into(),
                calls: vec![],
                hangs: None,
                budgets: vec![],
            }
        }
        fn hanging_on(step: Step) -> Self {
            Self {
                hangs: Some(step),
                ..Self::always_ok()
            }
        }
    }
    impl Exec for FakeExec {
        fn run(&mut self, command: &Command, remaining: Duration) -> StepOutcome {
            self.calls.push(command.step);
            self.budgets.push(remaining);
            if self.hangs == Some(command.step) {
                return StepOutcome {
                    step: command.step,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: "killed for running past the budget".into(),
                    expects_assertions: false,
                    timed_out: true,
                };
            }
            let code = self
                .script
                .iter_mut()
                .find(|(s, _)| *s == command.step)
                .and_then(|(_, codes)| {
                    if codes.is_empty() {
                        None
                    } else {
                        Some(codes.remove(0))
                    }
                })
                .unwrap_or(0);
            StepOutcome {
                step: command.step,
                exit_code: Some(code),
                stdout: if code != 0 {
                    self.stdout.clone()
                } else if command.step == Step::Test {
                    // A successful run phase has to look like one. The classifier now requires
                    // evidence that assertions actually ran, so a fake that returned nothing here
                    // would be modelling a leg that proves nothing — and the tests would be
                    // asserting against a shape the real harness never produces.
                    HARNESS_TEN_PASSED.to_string()
                } else {
                    String::new()
                },
                stderr: if code == 0 {
                    String::new()
                } else {
                    self.stderr.clone()
                },
                // Filled in by `run`, which is the only thing that knows the leg.
                expects_assertions: false,
                timed_out: false,
            }
        }
    }

    /// The measured shape of a client leg's run phase: the bin target with no `#[test]`, the
    /// hermetic assertions, and the `#[ignore]`d network probe.
    const HARNESS_TEN_PASSED: &str = "running 0 tests\n\
         test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n\
         running 10 tests\n\
         test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n\
         running 1 test\n\
         test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out\n";

    const COMPILE_ERROR: &str = r#"{"reason":"compiler-message","target":{"name":"canary-build-check"},"message":{"level":"error","code":{"code":"E0308"},"message":"mismatched types"}}"#;

    #[test]
    fn a_full_leg_runs_six_commands_in_order() {
        let leg = leg_named("new-project--stable");
        let cmds = commands(&leg);
        let steps: Vec<_> = cmds.iter().map(|c| c.step).collect();
        assert_eq!(
            steps,
            [
                Step::InstallToolchain,
                Step::GenerateLockfile,
                Step::Fetch,
                Step::Build,
                Step::TestNoRun,
                Step::Test,
            ]
        );
    }

    /// Every cargo command names its toolchain with `+<version>`, so the install has to come first.
    /// rustup 1.29 auto-installs a missing `+toolchain` with only a warning, so getting this wrong
    /// passes locally and depends on a convenience that `RUSTUP_AUTO_INSTALL=0` turns off.
    #[test]
    fn the_toolchain_is_installed_before_any_cargo_command() {
        for id in ["new-project--stable", "powerpc64", "release-profile"] {
            let cmds = commands(&leg_named(id));
            let first_cargo = cmds
                .iter()
                .position(|c| c.program == "cargo")
                .unwrap_or_else(|| panic!("{id} runs no cargo command"));
            let install = cmds
                .iter()
                .position(|c| c.program == "rustup" && c.args.contains(&"toolchain".to_string()))
                .unwrap_or_else(|| panic!("{id} never installs its toolchain"));
            assert!(
                install < first_cargo,
                "{id}: installs at {install} but runs cargo at {first_cargo}"
            );
            for c in cmds.iter().filter(|c| c.program == "cargo") {
                assert_eq!(
                    c.args.first().map(String::as_str),
                    Some(format!("+{}", leg_named(id).toolchain_version).as_str()),
                    "{id}: {:?} must pin its toolchain",
                    c.step
                );
            }
        }
    }

    /// Only the compile steps get `--message-format=json`. The run step must not, because its
    /// evidence is the harness's text output.
    #[test]
    fn json_is_requested_for_the_compile_steps_only() {
        for c in commands(&leg_named("new-project--stable")) {
            match c.step {
                Step::Build | Step::TestNoRun => assert!(c.json, "{:?} needs json", c.step),
                _ => assert!(!c.json, "{:?} must not ask for json", c.step),
            }
        }
    }

    /// `cargo generate-lockfile` is the one step that is *supposed* to reach the registry, so it
    /// must not be `--offline`; the three verdict-owning steps must be.
    #[test]
    fn the_verdict_owning_steps_are_offline_and_the_resolve_step_is_not() {
        for c in commands(&leg_named("new-project--stable")) {
            let offline = c.args.iter().any(|a| a == "--offline");
            match c.step {
                Step::Build | Step::TestNoRun | Step::Test => {
                    assert!(offline, "{:?} must be --offline", c.step)
                }
                Step::GenerateLockfile => {
                    assert!(!offline, "generate-lockfile reaches the registry")
                }
                _ => {}
            }
        }
    }

    #[test]
    fn a_build_only_leg_stops_after_build() {
        let cmds = commands(&leg_named("powerpc64"));
        let steps: Vec<_> = cmds.iter().map(|c| c.step).collect();
        assert_eq!(
            steps,
            [
                // The leg's own toolchain, then its cross target.
                Step::InstallToolchain,
                Step::InstallToolchain,
                Step::GenerateLockfile,
                Step::Fetch,
                Step::Build,
            ],
            "nothing can execute a powerpc binary here"
        );
        assert!(cmds.iter().any(|c| c.args.contains(&"--target".to_string())
            && c.args.contains(&"powerpc64-unknown-linux-gnu".to_string())));
    }

    /// Installing a toolchain does not bring its cross targets, and the build failure that follows
    /// looks exactly like a broken graph. Found by running a real powerpc64 leg.
    #[test]
    fn a_targeted_leg_adds_its_target_to_the_toolchain() {
        let cmds = commands(&leg_named("powerpc64"));
        let add = cmds
            .iter()
            .find(|c| c.program == "rustup" && c.args.contains(&"target".to_string()))
            .expect("a cross leg must add its target");
        assert!(add
            .args
            .contains(&"powerpc64-unknown-linux-gnu".to_string()));
        assert!(add.args.contains(&"--toolchain".to_string()));
        // Before the build, or it is useless.
        let add_at = cmds.iter().position(|c| std::ptr::eq(c, add)).unwrap();
        let build_at = cmds.iter().position(|c| c.step == Step::Build).unwrap();
        assert!(add_at < build_at);
        // And it is an InstallToolchain step, so it inherits the infra retry policy.
        assert_eq!(add.step, Step::InstallToolchain);
    }

    #[test]
    fn a_native_leg_adds_no_target() {
        assert!(!commands(&leg_named("new-project--stable"))
            .iter()
            .any(|c| c.args.contains(&"target".to_string()) && c.program == "rustup"));
    }

    #[test]
    fn the_release_leg_passes_release_and_the_others_do_not() {
        assert!(commands(&leg_named("release-profile"))
            .iter()
            .any(|c| c.args.contains(&"--release".to_string())));
        assert!(!commands(&leg_named("new-project--stable"))
            .iter()
            .any(|c| c.args.contains(&"--release".to_string())));
    }

    #[test]
    fn a_leg_whose_commands_all_pass_is_green_and_exits_zero() {
        let leg = leg_named("new-project--stable");
        let mut exec = FakeExec::always_ok();
        let mut clock = FakeClock::default();
        let r = run(&leg, &mut exec, &mut clock, BUDGET);
        assert!(r.is_green());
        assert_eq!(r.exit_code(), 0);
        assert_eq!(
            r.attempts.len(),
            commands(&leg).len(),
            "one attempt per command"
        );
        assert!(clock.slept.is_empty(), "a green leg never sleeps");
    }

    /// The core retry shape: three attempts, three minutes apart, so six minutes of waiting and then
    /// a verdict.
    #[test]
    fn an_infra_failure_gets_three_attempts_six_minutes_apart() {
        let leg = leg_named("new-project--stable");
        let mut exec = FakeExec::failing(Step::Fetch, vec![1, 1, 1], "", "network unreachable");
        let mut clock = FakeClock::default();
        let r = run(&leg, &mut exec, &mut clock, BUDGET);

        assert_eq!(r.verdict, Some(Verdict::InfraFail));
        assert_eq!(r.failed_step, Some(Step::Fetch));
        assert_eq!(r.stopped, Some(Stopped::AttemptsExhausted));
        assert_eq!(exec.calls.iter().filter(|s| **s == Step::Fetch).count(), 3);
        assert_eq!(
            clock.slept,
            [Duration::from_secs(180), Duration::from_secs(180)],
            "two waits between three attempts"
        );
        assert_eq!(r.elapsed, Duration::from_secs(360), "6 minutes");
        assert_eq!(r.exit_code(), 0, "INFRA_FAIL lets the window continue");
    }

    /// A step that recovers hands the next step a *full* set of attempts. Docking it would leave a
    /// leg hair-triggered after one slow start, which is the hole in a shared per-leg budget.
    ///
    /// Fetch fails once then recovers, so Build must still get all three of its own attempts.
    #[test]
    fn recovering_leaves_the_next_step_a_full_budget() {
        let leg = leg_named("new-project--stable");
        let mut exec = FakeExec {
            script: vec![(Step::Fetch, vec![1, 0]), (Step::Build, vec![1, 1, 1])],
            stderr: "network unreachable".into(),
            ..FakeExec::always_ok()
        };
        let mut clock = FakeClock::default();
        let r = run(&leg, &mut exec, &mut clock, BUDGET);

        assert_eq!(exec.calls.iter().filter(|s| **s == Step::Fetch).count(), 2);
        assert_eq!(
            exec.calls.iter().filter(|s| **s == Step::Build).count(),
            3,
            "Build was not docked by Fetch having stumbled earlier"
        );
        assert_eq!(r.verdict, Some(Verdict::InfraFail));
        assert_eq!(r.stopped, Some(Stopped::AttemptsExhausted));
        // One wait for Fetch plus two for Build, all inside the budget.
        assert_eq!(r.elapsed, Duration::from_secs(540));
        assert!(r.elapsed < BUDGET);
    }

    /// And the recovery case itself: a step that comes back on its last attempt lets the leg carry
    /// on and finish green.
    #[test]
    fn a_step_that_recovers_on_its_last_attempt_lets_the_leg_go_green() {
        let leg = leg_named("new-project--stable");
        let mut exec = FakeExec {
            script: vec![(Step::Build, vec![1, 1, 0])],
            stderr: "runner hiccup".into(),
            ..FakeExec::always_ok()
        };
        let mut clock = FakeClock::default();
        let r = run(&leg, &mut exec, &mut clock, BUDGET);
        assert!(r.is_green(), "recovered on attempt 3");
        assert!(exec.calls.contains(&Step::Test), "later steps still ran");
    }

    /// Exhaustion ends the leg, so no later step runs. This is what bounds the compounding.
    #[test]
    fn exhausting_a_step_means_no_later_step_runs() {
        let leg = leg_named("new-project--stable");
        let mut exec = FakeExec::failing(Step::Build, vec![101, 101], COMPILE_ERROR, "");
        let mut clock = FakeClock::default();
        let r = run(&leg, &mut exec, &mut clock, BUDGET);
        assert_eq!(r.verdict, Some(Verdict::CompileFail));
        assert!(
            !exec.calls.contains(&Step::TestNoRun) && !exec.calls.contains(&Step::Test),
            "the test steps must never run after a compile failure"
        );
        assert_eq!(r.exit_code(), 1, "a hard verdict stops the window");
    }

    /// A deterministic compile error is retried once, immediately — a retry only catches a flaky
    /// runner, and `--offline` means there is nothing else for it to catch.
    #[test]
    fn a_compile_error_is_retried_once_without_waiting() {
        let leg = leg_named("new-project--stable");
        let mut exec = FakeExec::failing(Step::Build, vec![101, 101], COMPILE_ERROR, "");
        let mut clock = FakeClock::default();
        run(&leg, &mut exec, &mut clock, BUDGET);
        assert_eq!(exec.calls.iter().filter(|s| **s == Step::Build).count(), 2);
        assert_eq!(clock.slept, [Duration::ZERO]);
    }

    /// A refused resolution waits a minute, not three: it is waiting for crates.io propagation
    /// rather than for a blip to clear.
    #[test]
    fn a_refused_resolution_waits_one_minute_and_retries_once() {
        let leg = leg_named("new-project--stable");
        let mut exec = FakeExec::failing(
            Step::GenerateLockfile,
            vec![101, 101],
            "",
            "error: failed to select a version for the requirement `aws-config = \"^999\"`",
        );
        let mut clock = FakeClock::default();
        let r = run(&leg, &mut exec, &mut clock, BUDGET);
        assert_eq!(r.verdict, Some(Verdict::ResolveFail));
        assert_eq!(clock.slept, [Duration::from_secs(60)]);
        assert_eq!(r.exit_code(), 1);
    }

    /// The case the deadline exists for, against the real executor and a real hanging process.
    ///
    /// Everything else here uses a fake clock, which proves the retry arithmetic and nothing about
    /// a command that never returns. This was `Command::output()` — it blocks until the child
    /// exits, so a stalled network read meant the leg never came back, GitHub killed the job at
    /// `timeout-minutes`, and a killed job runs no further steps and emits *nothing*. Silence is
    /// worse than a red verdict, because an absent datapoint cannot raise an alarm.
    #[cfg(unix)]
    #[test]
    fn a_hanging_command_is_killed_and_reported_rather_than_waited_on() {
        let mut exec = RealExec {
            dir: std::env::temp_dir(),
            verbose: false,
        };
        let command = Command {
            step: Step::GenerateLockfile,
            program: "sleep".into(),
            args: vec!["120".into()],
            json: false,
        };

        let started = std::time::Instant::now();
        let outcome = exec.run(&command, Duration::from_millis(600));
        let took = started.elapsed();

        assert!(outcome.timed_out, "it must report that it was killed");
        assert!(
            took < Duration::from_secs(10),
            "it waited {took:?} on a 120-second sleep given 600ms"
        );
        assert!(
            outcome.stderr.contains("killed this command"),
            "{outcome:?}"
        );

        // And the verdict blames the runner, not the release.
        let (verdict, diagnosis) = classify::classify(&outcome).expect("a kill is not a pass");
        assert_eq!(verdict, Verdict::InfraFail);
        assert!(
            diagnosis
                .summary
                .unwrap()
                .contains("ran past the leg's budget"),
            "the page has to say why there is no verdict"
        );
    }

    /// A command that produces more output than a pipe buffer holds must still be waited on
    /// correctly. Polling `try_wait` without draining the pipes concurrently deadlocks: the child
    /// blocks writing, so it never exits, so the poll never ends. `cargo build
    /// --message-format=json` emits megabytes, so this is the ordinary path.
    #[cfg(unix)]
    #[test]
    fn a_command_that_floods_its_pipe_does_not_deadlock() {
        let mut exec = RealExec {
            dir: std::env::temp_dir(),
            verbose: false,
        };
        let command = Command {
            step: Step::Build,
            program: "sh".into(),
            // ~2 MiB on stdout, far past the 64 KiB pipe buffer.
            args: vec![
                "-c".into(),
                "i=0; while [ $i -lt 2048 ]; do printf '%01024d\\n' $i; i=$((i+1)); done".into(),
            ],
            json: true,
        };

        let outcome = exec.run(&command, Duration::from_secs(30));
        assert!(
            !outcome.timed_out,
            "it should have finished, not been killed"
        );
        assert_eq!(outcome.exit_code, Some(0));
        assert!(
            outcome.stdout.len() > 2_000_000,
            "captured {} bytes; the pipe should have been drained in full",
            outcome.stdout.len()
        );
    }

    /// Each command is handed what is left of the leg's budget, not a fresh constant, so a leg
    /// cannot overrun however it fails.
    #[test]
    fn each_command_gets_only_the_budget_that_remains() {
        let leg = leg_named("new-project--stable");
        let mut exec = FakeExec::failing(Step::Fetch, vec![1, 1, 0], "", "network unreachable");
        let mut clock = FakeClock::default();
        run(&leg, &mut exec, &mut clock, BUDGET);

        assert!(exec.budgets.len() >= 4, "{:?}", exec.budgets);
        for w in exec.budgets.windows(2) {
            assert!(
                w[1] <= w[0],
                "the budget must only ever shrink: {:?}",
                exec.budgets
            );
        }
        assert_eq!(exec.budgets[0], BUDGET, "the first command gets all of it");
        assert!(
            *exec.budgets.last().unwrap() < BUDGET,
            "and later ones get less, after the retry waits: {:?}",
            exec.budgets
        );
    }

    /// A hang anywhere in the leg ends it with a verdict, rather than leaving later steps to run on
    /// a budget that is already spent.
    #[test]
    fn a_hanging_step_ends_the_leg_with_infra_fail() {
        let leg = leg_named("new-project--stable");
        let mut exec = FakeExec::hanging_on(Step::Build);
        let mut clock = FakeClock::default();
        let r = run(&leg, &mut exec, &mut clock, BUDGET);

        assert_eq!(r.verdict, Some(Verdict::InfraFail));
        assert_eq!(r.failed_step, Some(Step::Build));
        assert!(
            !exec.calls.contains(&Step::TestNoRun) && !exec.calls.contains(&Step::Test),
            "nothing after the hang should have run: {:?}",
            exec.calls
        );
        assert_eq!(r.exit_code(), 0, "infra must not stop the window");
    }

    /// The deadline must produce a verdict rather than let GitHub kill the job, because a killed job
    /// runs no further steps and so emits nothing.
    #[test]
    fn the_deadline_stops_retrying_and_still_reports() {
        let leg = leg_named("new-project--stable");
        // Enough room for one attempt but not for a three-minute wait.
        let budget = Duration::from_secs(100);
        let mut exec = FakeExec::failing(Step::Fetch, vec![1, 1, 1], "", "network unreachable");
        let mut clock = FakeClock::default();
        let r = run(&leg, &mut exec, &mut clock, budget);

        assert_eq!(r.stopped, Some(Stopped::DeadlineReached));
        assert_eq!(r.verdict, Some(Verdict::InfraFail));
        assert_eq!(
            exec.calls.iter().filter(|s| **s == Step::Fetch).count(),
            1,
            "it did not sleep past the deadline"
        );
        assert!(clock.slept.is_empty());
        assert!(r.elapsed < budget);
    }

    /// 6 minutes of retry fits inside the 14-minute budget with room for the work, which is the
    /// arithmetic that keeps a leg under the watcher's 15-minute tick.
    #[test]
    fn one_exhausted_infra_step_fits_well_inside_the_budget() {
        let worst = Retry::for_verdict(Verdict::InfraFail);
        let waiting = worst.interval * (worst.attempts - 1);
        assert_eq!(waiting, Duration::from_secs(360));
        assert!(
            waiting * 2 < BUDGET,
            "two exhausted infra steps should still fit"
        );
        assert!(BUDGET < Duration::from_secs(15 * 60), "under the tick");
    }

    #[test]
    fn the_report_logs_every_attempt_with_its_verdict_and_timing() {
        let leg = leg_named("new-project--stable");
        let mut exec = FakeExec::failing(Step::Fetch, vec![1, 0], "", "network unreachable");
        let mut clock = FakeClock::default();
        let r = run(&leg, &mut exec, &mut clock, BUDGET);
        let fetches: Vec<_> = r
            .attempts
            .iter()
            .filter(|a| a.step == Step::Fetch)
            .collect();
        assert_eq!(fetches.len(), 2);
        assert_eq!(fetches[0].attempt, 1);
        assert_eq!(fetches[0].verdict, Some(Verdict::InfraFail));
        assert_eq!(fetches[0].at, Duration::ZERO);
        assert!(fetches[1].succeeded);
        assert_eq!(fetches[1].at, Duration::from_secs(180));
        assert!(r.is_green());
    }

    /// Only `INFRA_FAIL` and green exit 0. Everything else stops the floor tier.
    #[test]
    fn exit_codes_carry_the_stop_decision() {
        let mk = |v: Option<Verdict>| Report {
            leg_id: "x".into(),
            verdict: v,
            failed_step: None,
            diagnosis: None,
            stopped: None,
            attempts: vec![],
            elapsed: Duration::ZERO,
        };
        assert_eq!(mk(None).exit_code(), 0);
        assert_eq!(mk(Some(Verdict::InfraFail)).exit_code(), 0);
        for v in [
            Verdict::ResolveFail,
            Verdict::CompileFail,
            Verdict::HarnessFail,
            Verdict::RuntimeFail,
        ] {
            assert_eq!(mk(Some(v)).exit_code(), 1, "{v} must stop the window");
        }
    }
}
