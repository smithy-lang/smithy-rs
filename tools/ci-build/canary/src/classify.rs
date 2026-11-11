/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Turning a failed command into a verdict.
//!
//! This is the piece where a wrong answer costs the most: every verdict pages at Sev2, so the
//! classification does not decide *whether* someone is woken, it decides *what they are told to look
//! at*. Reporting a real compile break as `INFRA_FAIL` sends the oncall to the runner instead of to
//! the published graph.
//!
//! # The rule
//!
//! **A hard verdict requires positive evidence; everything else is `INFRA_FAIL`.**
//!
//! That is deliberately not an enumeration of infra failure strings. Such a list is an arms race
//! against cargo, rustup, the registry and the runner, and anything it misses becomes a
//! misattributed page. Instead each step names the evidence that would make a failure attributable
//! to the graph, and the absence of that evidence is itself the signal.
//!
//! # Why the evidence is trustworthy
//!
//! The three verdict-owning commands run `--offline`, after `cargo fetch --locked` has already
//! populated the cache. They *structurally cannot* reach the network, so "no compiler diagnostic"
//! cannot mean "the registry was slow" — it means the runner broke.
//!
//! Every discriminator below was measured, not assumed:
//!
//! | Step | Evidence | Verdict |
//! |---|---|---|
//! | `cargo generate-lockfile` | `failed to select a version` | `RESOLVE_FAIL` |
//! | `rustup toolchain install` | none possible | `INFRA_FAIL` |
//! | `cargo fetch --locked` | none possible | `INFRA_FAIL` |
//! | `cargo build --offline` | a `compiler-message` at `level: "error"` | `COMPILE_FAIL` |
//! | `cargo test --no-run --offline` | a `compiler-message` at `level: "error"` | `HARNESS_FAIL` |
//! | `cargo test --offline` | `test result: FAILED` or `panicked at` | `RUNTIME_FAIL` |

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::lockfile::{Duplicated, Resolved};

/// The five verdicts a leg can report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Verdict {
    /// The graph could not be resolved at all.
    ResolveFail,
    /// `cargo build` failed with a genuine compiler diagnostic — what a consumer compiles, so this
    /// is the consumer-impacting one.
    CompileFail,
    /// The test binary failed to compile, so no assertion ran. Less severe than `COMPILE_FAIL`
    /// because nothing a consumer compiles is implicated — only test-only dependencies.
    HarnessFail,
    /// The binary compiled, ran, and an assertion failed.
    RuntimeFail,
    /// Not attributable to the published graph: fetch, DNS, toolchain install, the runner.
    InfraFail,
}

impl Verdict {
    /// Whether a floor hour ending in this verdict should stop the remaining hours.
    ///
    /// `INFRA_FAIL` does not: a crates.io hiccup or a DNS blip is expected to clear and the window
    /// should survive it. Every hard verdict does: the team has already been paged from this run, and
    /// later hours would dedupe into the same ticket and page nobody, so continuing buys nothing.
    pub fn stops_the_window(self) -> bool {
        self != Verdict::InfraFail
    }

    /// Whether this verdict blames the published graph. The complement of `INFRA_FAIL`.
    pub fn is_hard(self) -> bool {
        self != Verdict::InfraFail
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Verdict::ResolveFail => "RESOLVE_FAIL",
            Verdict::CompileFail => "COMPILE_FAIL",
            Verdict::HarnessFail => "HARNESS_FAIL",
            Verdict::RuntimeFail => "RUNTIME_FAIL",
            Verdict::InfraFail => "INFRA_FAIL",
        };
        f.write_str(s)
    }
}

/// The leg's commands, in the order a leg runs them. See [`crate::leg::commands`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Step {
    GenerateLockfile,
    InstallToolchain,
    Fetch,
    Build,
    TestNoRun,
    Test,
}

impl Step {
    /// Whether the step can reach the network. Only the first three can, which is what makes the
    /// absence of a compiler diagnostic in the last three meaningful.
    pub fn can_reach_the_network(self) -> bool {
        matches!(
            self,
            Step::GenerateLockfile | Step::InstallToolchain | Step::Fetch
        )
    }
}

/// What a command did.
#[derive(Debug, Clone)]
pub struct StepOutcome {
    pub step: Step,
    /// `None` when the process was killed by a signal, which is itself infra.
    pub exit_code: Option<i32>,
    /// For `Build` and `TestNoRun`, cargo's `--message-format=json` lines. For `Test`, the harness's
    /// text output.
    pub stdout: String,
    pub stderr: String,
    /// Whether this leg's body declares assertions, so a run phase that executed none can be told
    /// apart from one that had none to execute. A stub body legitimately runs zero tests.
    pub expects_assertions: bool,
    /// The command was killed for running past the leg's remaining budget. Distinguished from an
    /// ordinary failure because a killed command's output is arbitrary: it may carry half a
    /// diagnostic, or none, and neither says anything about the published graph.
    pub timed_out: bool,
}

impl StepOutcome {
    pub fn succeeded(&self) -> bool {
        self.exit_code == Some(0)
    }
}

/// What a responder needs in order to know where to look, carried on the dimensioned metric and
/// into the ticket.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnosis {
    /// Compiler error codes, in order of appearance: `E0308`, `E0432`, …
    #[serde(default)]
    pub codes: Vec<String>,
    /// The first error's rendered message, truncated.
    pub summary: Option<String>,
    /// The cargo target the first error came from, which distinguishes a break in the consumer's
    /// own code from one in a test target.
    pub target: Option<String>,
    /// The resolved version of [`Self::target`]'s package, so a page says `aws-config 1.10.1`
    /// rather than `aws_config`. Absent when there is no lockfile to read, which is every
    /// `RESOLVE_FAIL`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_version: Option<String>,
    /// Crates under test that resolved to more than one version in the same graph.
    ///
    /// This is the version pair a page is supposed to name. See [`crate::lockfile`] for why it is
    /// taken from the lockfile rather than the compiler's prose, and why it is only ever evidence
    /// alongside a verdict reached some other way.
    ///
    /// `default` is load-bearing, not tidiness. `skip_serializing_if` omits this when it is empty,
    /// which is the common case — most compile breaks involve no duplicated crate at all — and
    /// without a default serde refuses to read such a report back. The leg that wrote it would then
    /// be counted as having sent no report, which is the signal reserved for a leg that died
    /// silently. So the record would quietly lose exactly the red legs with the simplest
    /// diagnoses, while reporting them as a defect in the canary. Found by folding a real run's
    /// reports, not by a test.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub duplicate_versions: Vec<Duplicated>,
}

impl Diagnosis {
    /// Add what the resolved graph says, given the leg's `Cargo.lock`.
    ///
    /// Separate from [`classify`] because the classifier sees only a command's output and must stay
    /// that way: a verdict has to be decided from the evidence the step itself produced, never from
    /// a file on disk that a later step could have rewritten.
    ///
    /// `None` and an unparseable lockfile both leave the fields empty rather than failing the leg.
    /// A lockfile cargo just wrote should always parse, and if it somehow does not, the cost is a
    /// less informative ticket — whereas turning that into an error would discard a real verdict
    /// over a presentation detail.
    pub fn with_resolved_graph(mut self, lock: Option<&str>) -> Self {
        let Some(resolved) = lock.and_then(|l| Resolved::parse(l).ok()) else {
            return self;
        };
        self.target_version = self
            .target
            .as_deref()
            .and_then(|t| resolved.version_of_target(t));
        self.duplicate_versions = resolved.duplicated_crates_under_test();
        self
    }
}

/// Classify a failed step.
///
/// Returns `None` for a step that succeeded, so the caller can keep walking the list.
pub fn classify(outcome: &StepOutcome) -> Option<(Verdict, Diagnosis)> {
    if outcome.succeeded() {
        // A green run phase has to rest on evidence that something ran. `cargo test` exits 0 when
        // it executes nothing at all, so if the body's assertions ever stopped being rendered, or a
        // filter excluded them, every client leg would report green having checked nothing — the
        // exact false green this crate exists to prevent. Found because the first real workflow run
        // had no way to show, from the job log, whether ten assertions had run or none.
        if outcome.step == Step::Test
            && outcome.expects_assertions
            && assertions_passed(&outcome.stdout) == 0
        {
            return Some((
                Verdict::InfraFail,
                Diagnosis {
                    codes: vec![],
                    summary: Some(
                        "the run phase passed without executing a single assertion, so this leg \
                         proves nothing about the release"
                            .to_string(),
                    ),
                    ..Default::default()
                },
            ));
        }
        return None;
    }
    // A killed command is the runner, never the graph, and it has to be decided before anything
    // reads the output: a `cargo build` killed mid-flight may have already emitted a real-looking
    // diagnostic, which would otherwise be read as a `COMPILE_FAIL` and page about a break that was
    // never established.
    if outcome.timed_out {
        return Some((
            Verdict::InfraFail,
            Diagnosis {
                codes: vec![],
                summary: Some(format!(
                    "{:?} ran past the leg's budget and was killed, so this leg reached no verdict \
                     about the release",
                    outcome.step
                )),
                ..Default::default()
            },
        ));
    }

    let combined = format!("{}\n{}", outcome.stdout, outcome.stderr);
    let verdict = match outcome.step {
        // Cargo could not pick a version. Measured string:
        //   error: failed to select a version for the requirement `aws-smithy-types = "^999"`
        // A missing crate reports differently but is the same class of failure: the graph the
        // consumer asked for does not exist.
        Step::GenerateLockfile => {
            if resolution_was_refused(&combined) {
                Verdict::ResolveFail
            } else {
                Verdict::InfraFail
            }
        }
        // Neither step can implicate the graph. rustup is not our dependency tree, and a fetch
        // failure is by definition the registry or the network.
        Step::InstallToolchain | Step::Fetch => Verdict::InfraFail,
        Step::Build => match attributable_errors(&outcome.stdout) {
            Attribution::Graph => Verdict::CompileFail,
            Attribution::Toolchain | Attribution::None => Verdict::InfraFail,
        },
        Step::TestNoRun => match attributable_errors(&outcome.stdout) {
            Attribution::Graph => Verdict::HarnessFail,
            Attribution::Toolchain | Attribution::None => Verdict::InfraFail,
        },
        // By this point `cargo test --no-run` has already succeeded, so the compile is a no-op and
        // the run phase emits no compiler-message at all — measured. The evidence is the harness's.
        Step::Test => {
            if connector_failed(&combined) {
                Verdict::InfraFail
            } else if assertions_ran_and_failed(&combined) {
                Verdict::RuntimeFail
            } else {
                Verdict::InfraFail
            }
        }
    };
    Some((verdict, diagnose(outcome)))
}

/// An assertion that reaches the network says it could not get a response at all.
///
/// Without this, every such failure is a `RUNTIME_FAIL`: a hard verdict, which stops the remaining
/// hours of the window and pages saying the published crates are broken. An unreachable network is
/// a property of the runner, so that attribution is wrong and the window should continue.
///
/// **The classifier cannot make this call on its own, and does not try.** From outside the harness,
/// a failed test is a failed test; nothing in the output distinguishes "the network was down" from
/// "the published client can no longer do TLS". So the *assertion* decides, having had the SDK error
/// in hand, and says so with this marker. Any assertion that reaches the network opts in by panicking
/// with it.
///
/// The bias this encodes is deliberate. It trades away detection of one unproven defect class — a
/// broken platform certificate store, which could not be induced and so may not be detectable here
/// at all — to avoid stopping a 24-hour window on a network blip, which is demonstrably what this
/// marker fires on. A real break would still page, every hour, with the marker's own text naming the
/// connector; the recurrence is what would distinguish it, and that lives above a single leg.
fn connector_failed(output: &str) -> bool {
    output.contains("CANARY_CONNECTOR_FAIL")
}

/// Cargo refused to resolve, as opposed to failing to reach the registry.
fn resolution_was_refused(output: &str) -> bool {
    const REFUSALS: &[&str] = &[
        // Measured: an unsatisfiable requirement on a crate that exists.
        "failed to select a version",
        // A crate that does not exist at all.
        "no matching package",
        // A requirement no version satisfies, reported as a conflict.
        "failed to resolve",
        // MSRV-aware resolution refusing on rust-version. Should not arise — the manifests stay on
        // resolver 2 — but if it ever does it is the graph, not the runner.
        "requires rustc",
    ];
    REFUSALS.iter().any(|m| output.contains(m))
}

/// The test harness ran and reported a failure, rather than the runner dying before it could.
/// How many assertions actually passed, summed across the leg's test binaries.
///
/// A client leg runs three: the bin target with no `#[test]` at all, the hermetic assertions, and
/// the network probe which is `#[ignore]`d. So the measured shape is a `0 passed`, a `10 passed` and
/// a `0 passed; 0 failed; 1 ignored` — and only the total is meaningful.
fn assertions_passed(stdout: &str) -> u32 {
    stdout
        .lines()
        .filter_map(|l| l.trim().strip_prefix("test result: ok. "))
        .filter_map(|rest| rest.split(" passed").next()?.parse::<u32>().ok())
        .sum()
}

fn assertions_ran_and_failed(output: &str) -> bool {
    // Measured shapes:
    //   test result: FAILED. 1 passed; 1 failed; 0 ignored; ...
    //   test an_assertion_that_fails ... FAILED
    //   thread 'an_assertion_that_fails' (17190) panicked at tests/hermetic.rs:4:32:
    output.contains("test result: FAILED")
        || output.contains("... FAILED")
        || output.contains("panicked at")
}

/// What a build's diagnostics can be blamed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Attribution {
    /// At least one error that implicates the dependency graph.
    Graph,
    /// Errors, but all of them say the *compiler* is wrong rather than the code.
    Toolchain,
    /// No errors at all.
    None,
}

/// Decide whether a build's diagnostics blame the graph or the toolchain.
///
/// This is the one place the module enumerates error strings, and it is worth justifying because the
/// module docs argue against exactly that. The general case — listing every way infra can fail — is
/// an arms race against cargo, rustup, the registry and the runner. This is the opposite and much
/// smaller case: a fixed, stable set of *compiler* diagnostics that mean the sysroot is wrong.
///
/// It was found by running a real cross-compiled leg without `rustup target add`: the build failed
/// with `E0463: can't find crate for core` on nineteen crates. That is a genuine
/// `compiler-message` at `level: "error"`, so the discriminator above classified it `COMPILE_FAIL`
/// and would have paged claiming a published crate does not build. The cause was a missing target.
///
/// Note the narrowness: `E0463` for an ordinary crate *is* a graph failure — a missing dependency —
/// so the sysroot crate names are part of the test, not just the code.
fn attributable_errors(stdout: &str) -> Attribution {
    const SYSROOT_CRATES: &[&str] = &["core", "std", "alloc", "proc_macro", "test"];
    let mut saw_error = false;
    let mut saw_graph_error = false;

    for msg in compiler_errors(stdout) {
        saw_error = true;
        let code = msg
            .get("message")
            .and_then(|m| m.get("code"))
            .and_then(|c| c.get("code"))
            .and_then(|c| c.as_str())
            .unwrap_or_default();
        let text = msg
            .get("message")
            .and_then(|m| m.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or_default();

        let sysroot = match code {
            // "can't find crate for `core`" — the target's rust-std is not installed.
            "E0463" => SYSROOT_CRATES
                .iter()
                .any(|c| text.contains(&format!("`{c}`"))),
            // Crate compiled by an incompatible version of rustc: a mixed or partial sysroot.
            "E0461" | "E0514" | "E0519" => true,
            _ => false,
        };
        if !sysroot {
            saw_graph_error = true;
        }
    }

    match (saw_error, saw_graph_error) {
        (false, _) => Attribution::None,
        (true, false) => Attribution::Toolchain,
        (true, true) => Attribution::Graph,
    }
}

/// Cargo's `compiler-message` records at `level: "error"`.
///
/// `level` also takes `warning` and `failure-note` — the latter is the "For more information about
/// this error" line, measured as a second `compiler-message` on every failing build. Counting it
/// would make a warning-only build look like a failure.
fn compiler_errors(stdout: &str) -> impl Iterator<Item = serde_json::Value> + '_ {
    stdout.lines().filter_map(|line| {
        let line = line.trim();
        if !line.starts_with('{') {
            return None;
        }
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        if v.get("reason")?.as_str()? != "compiler-message" {
            return None;
        }
        if v.get("message")?.get("level")?.as_str()? != "error" {
            return None;
        }
        Some(v)
    })
}

fn diagnose(outcome: &StepOutcome) -> Diagnosis {
    let mut d = Diagnosis::default();
    for msg in compiler_errors(&outcome.stdout) {
        if let Some(code) = msg
            .get("message")
            .and_then(|m| m.get("code"))
            .and_then(|c| c.get("code"))
            .and_then(|c| c.as_str())
        {
            if !d.codes.iter().any(|c| c == code) {
                d.codes.push(code.to_string());
            }
        }
        if d.summary.is_none() {
            d.summary = msg
                .get("message")
                .and_then(|m| m.get("message"))
                .and_then(|m| m.as_str())
                .map(truncate);
            d.target = msg
                .get("target")
                .and_then(|t| t.get("name"))
                .and_then(|n| n.as_str())
                .map(str::to_string);
        }
    }
    if d.summary.is_none() && outcome.step == Step::Test {
        d.summary = summarize_test_run(&outcome.stdout);
    }
    if d.summary.is_none() {
        d.summary = summarize_stderr(&outcome.stderr);
    }
    d
}

/// The failing assertion, named, for a run-phase failure.
///
/// The run phase emits no `compiler-message`, and cargo's stderr says only
/// `error: test failed, to rerun pass --test hermetic`, which tells a responder nothing. The
/// assertions are individually named precisely so a page can say which capability broke, and that
/// information is on *stdout*, in the harness's own output. Found by inducing `RUNTIME_FAIL`.
fn summarize_test_run(stdout: &str) -> Option<String> {
    let failed: Vec<&str> = stdout
        .lines()
        .filter_map(|l| l.trim().strip_prefix("test ")?.strip_suffix(" ... FAILED"))
        .collect();
    if failed.is_empty() {
        return None;
    }

    // The panic message, which is what says *how* it broke. `assertion ... failed` covers
    // `assert!` and `assert_eq!`. Otherwise take the line *after* `panicked at`, because that is
    // where Rust puts the message:
    //
    //     thread 'NAME' (ID) panicked at FILE:LINE:COL:
    //     the message
    //
    // Reporting the `panicked at` line itself instead gives a thread name and a file position and
    // no reason — which is what a real connector failure produced before this.
    let lines: Vec<&str> = stdout.lines().map(str::trim).collect();
    let why = lines
        .iter()
        .find(|l| l.starts_with("assertion "))
        .or_else(|| {
            lines
                .iter()
                .position(|l| l.contains("panicked at"))
                .and_then(|i| lines[i + 1..].iter().find(|l| !l.is_empty()))
        })
        .copied();

    let names = failed.join(", ");
    Some(truncate(&match why {
        Some(why) => format!("{names}: {why}"),
        None => format!("{names} failed"),
    }))
}

/// The one line of stderr worth putting on a ticket, when there is no compiler diagnostic.
///
/// Prefer the first `error:` line, because that is the failure itself. Taking the *last* line
/// instead looks right and is wrong: a cargo resolve failure ends with a `help:` block, so a
/// `RESOLVE_FAIL` page read `aws-config = { version = "0.0.26-alpha" }` — a suggestion, presented
/// where the responder expects the error. Found by inducing the verdict rather than by unit tests.
///
/// Measured on both tools the canary runs: cargo puts the failure on its first `error:` line and
/// follows it with context and `help:`, and rustup puts its entire causal chain *inside* one
/// `error:` line. One rule covers both.
fn summarize_stderr(stderr: &str) -> Option<String> {
    let lines = || stderr.lines().map(str::trim).filter(|l| !l.is_empty());
    // A continuation of some earlier message, never the message itself.
    let is_aside = |l: &str| l.starts_with("help:") || l.starts_with("note:");

    lines()
        .find(|l| l.starts_with("error:"))
        .or_else(|| lines().rfind(|l| !is_aside(l)))
        .or_else(|| lines().next_back())
        .map(truncate)
}

fn truncate(s: &str) -> String {
    const MAX: usize = 200;
    if s.chars().count() <= MAX {
        return s.to_string();
    }
    let mut out: String = s.chars().take(MAX).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(step: Step, stdout: &str, stderr: &str) -> StepOutcome {
        StepOutcome {
            step,
            exit_code: Some(101),
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            expects_assertions: true,
            timed_out: false,
        }
    }

    /// A successful step, for the rules that only apply when nothing failed.
    fn green(step: Step, stdout: &str, expects_assertions: bool) -> StepOutcome {
        StepOutcome {
            step,
            exit_code: Some(0),
            stdout: stdout.to_string(),
            stderr: String::new(),
            expects_assertions,
            timed_out: false,
        }
    }

    const TEN_PASSED: &str = "running 0 tests\n\
         test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n\
         running 10 tests\n\
         test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n\
         running 1 test\n\
         test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out\n";

    /// The measured shape of a client leg's run phase: three test binaries, only one of which has
    /// assertions, and an ignored probe. Only the total matters.
    #[test]
    fn a_green_run_phase_with_assertions_has_no_verdict() {
        assert_eq!(assertions_passed(TEN_PASSED), 10);
        assert!(classify(&green(Step::Test, TEN_PASSED, true)).is_none());
    }

    /// The false green this guards. `cargo test` exits 0 when it runs nothing, so if the body's
    /// assertions stopped being rendered every client leg would report green having checked nothing.
    #[test]
    fn a_green_run_phase_that_asserted_nothing_is_infra_not_green() {
        let nothing = "running 0 tests\n\
             test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n";
        let (v, d) = classify(&green(Step::Test, nothing, true)).expect("must not be green");
        assert_eq!(v, Verdict::InfraFail);
        assert!(
            d.summary
                .unwrap()
                .contains("without executing a single assertion"),
            "and it must say so"
        );
    }

    /// A stub leg has no `#[test]` anywhere, so running zero is correct rather than suspicious.
    #[test]
    fn a_stub_leg_running_no_assertions_is_still_green() {
        let nothing = "running 0 tests\n\
             test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n";
        assert!(classify(&green(Step::Test, nothing, false)).is_none());
    }

    /// The check is scoped to the run phase. A green build emits json, not harness output, and must
    /// not be second-guessed for lacking a `test result` line.
    #[test]
    fn the_assertion_check_does_not_apply_to_the_compile_steps() {
        for step in [
            Step::Build,
            Step::TestNoRun,
            Step::Fetch,
            Step::GenerateLockfile,
        ] {
            assert!(
                classify(&green(step, "", true)).is_none(),
                "{step:?} has no assertions to run"
            );
        }
    }

    /// Captured verbatim from `cargo build --offline --message-format=json` on a `let _: u32 =
    /// "not a number";`. Note the second record: `failure-note`, not `error`.
    const COMPILE_ERROR_JSON: &str = r#"
{"reason":"compiler-artifact","target":{"name":"aws-smithy-types"}}
{"reason":"compiler-message","target":{"name":"clsfy","kind":["bin"]},"message":{"level":"error","code":{"code":"E0308"},"message":"mismatched types","rendered":"error[E0308]: mismatched types\n --> src/main.rs:1:26\n"}}
{"reason":"compiler-message","target":{"name":"clsfy","kind":["bin"]},"message":{"level":"failure-note","code":null,"message":"For more information about this error, try `rustc --explain E0308`."}}
{"reason":"build-finished","success":false}
"#;

    /// Same shape but the error is in the test target, which is what `cargo test --no-run` reports.
    const HARNESS_ERROR_JSON: &str = r#"
{"reason":"compiler-message","target":{"name":"hermetic","kind":["test"]},"message":{"level":"error","code":{"code":"E0308"},"message":"mismatched types"}}
{"reason":"compiler-message","target":{"name":"hermetic","kind":["test"]},"message":{"level":"failure-note","code":null,"message":"For more information"}}
{"reason":"build-finished","success":false}
"#;

    /// Captured from `cargo test --offline` with one passing and one failing assertion.
    const RUNTIME_FAILURE_TEXT: &str = "
test passes ... ok
test an_assertion_that_fails ... FAILED

failures:

---- an_assertion_that_fails stdout ----
thread 'an_assertion_that_fails' (17190) panicked at tests/hermetic.rs:4:32:
assertion `left == right` failed: arithmetic is broken
  left: 2
 right: 3

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
";

    /// Captured from `cargo generate-lockfile` with `aws-smithy-types = "999"`.
    const RESOLVE_REFUSAL: &str = r#"    Updating crates.io index
error: failed to select a version for the requirement `aws-smithy-types = "^999"`
candidate versions found which didn't match: 1.9.4, 1.9.3, 1.9.1, ...
"#;

    #[test]
    fn a_successful_step_has_no_verdict() {
        let mut o = outcome(Step::Build, "", "");
        o.exit_code = Some(0);
        assert!(classify(&o).is_none());
    }

    /// Every shape a real `report.json` takes has to read back, because the durable record is built
    /// by deserializing them and a field that fails to parse is indistinguishable from a leg that
    /// never reported. The empty-diagnosis case is the one that bit: `skip_serializing_if` omits
    /// `duplicate_versions` whenever a break involves no duplicated crate, which is most of them.
    #[test]
    fn a_diagnosis_round_trips_whether_or_not_it_names_versions() {
        let bare = Diagnosis {
            codes: vec!["E0308".into()],
            summary: Some("mismatched types".into()),
            target: Some("aws_config".into()),
            target_version: None,
            duplicate_versions: vec![],
        };
        let json = serde_json::to_string(&bare).unwrap();
        assert!(
            !json.contains("duplicate_versions"),
            "the empty case is omitted on the way out: {json}"
        );
        assert_eq!(
            serde_json::from_str::<Diagnosis>(&json).expect("and must still read back"),
            bare
        );

        let full = Diagnosis {
            target_version: Some("1.10.1".into()),
            duplicate_versions: vec![crate::lockfile::Duplicated {
                name: "aws-smithy-schema".into(),
                versions: vec!["0.2.1".into(), "1.2.0".into()],
            }],
            ..bare
        };
        let json = serde_json::to_string(&full).unwrap();
        assert_eq!(serde_json::from_str::<Diagnosis>(&json).unwrap(), full);
    }

    /// A report written before these fields existed must still read, so that adding a field to the
    /// diagnosis never silently drops older records.
    #[test]
    fn a_diagnosis_without_the_version_fields_still_reads() {
        let old = r#"{"codes":["E0308"],"summary":"mismatched types","target":"aws_config"}"#;
        let d: Diagnosis = serde_json::from_str(old).expect("an older report must still parse");
        assert_eq!(d.codes, ["E0308"]);
        assert!(d.target_version.is_none());
        assert!(d.duplicate_versions.is_empty());
    }

    /// The enrichment must never cost a verdict. A lockfile that cannot be read, or is not there,
    /// leaves a diagnosis exactly as the classifier built it.
    #[test]
    fn an_unreadable_lockfile_leaves_the_diagnosis_untouched() {
        let (_, d) = classify(&outcome(Step::Build, COMPILE_ERROR_JSON, "")).unwrap();
        for lock in [None, Some("this is not toml {{{"), Some("")] {
            let enriched = d.clone().with_resolved_graph(lock);
            assert_eq!(
                enriched.summary.as_deref(),
                Some("mismatched types"),
                "{lock:?} must not disturb the diagnosis"
            );
            assert_eq!(enriched.codes, ["E0308"]);
            assert!(enriched.target_version.is_none());
            assert!(enriched.duplicate_versions.is_empty());
        }
    }

    #[test]
    fn a_compiler_diagnostic_on_build_is_a_compile_fail() {
        let (v, d) = classify(&outcome(Step::Build, COMPILE_ERROR_JSON, "")).unwrap();
        assert_eq!(v, Verdict::CompileFail);
        assert_eq!(d.codes, ["E0308"]);
        assert_eq!(d.summary.as_deref(), Some("mismatched types"));
        assert_eq!(d.target.as_deref(), Some("clsfy"));
    }

    #[test]
    fn a_compiler_diagnostic_on_test_no_run_is_a_harness_fail() {
        let (v, d) = classify(&outcome(Step::TestNoRun, HARNESS_ERROR_JSON, "")).unwrap();
        assert_eq!(v, Verdict::HarnessFail);
        assert_eq!(
            d.target.as_deref(),
            Some("hermetic"),
            "the target tells a responder it was test-only"
        );
    }

    #[test]
    fn a_failing_assertion_is_a_runtime_fail() {
        let (v, _) = classify(&outcome(Step::Test, RUNTIME_FAILURE_TEXT, "")).unwrap();
        assert_eq!(v, Verdict::RuntimeFail);
    }

    #[test]
    fn a_refused_resolution_is_a_resolve_fail() {
        let (v, _) = classify(&outcome(Step::GenerateLockfile, "", RESOLVE_REFUSAL)).unwrap();
        assert_eq!(v, Verdict::ResolveFail);
    }

    /// Measured: `RUSTUP_DIST_SERVER=https://nonexistent.invalid.example rustup toolchain install`
    /// yields a DNS error and no `compiler-message` at all. The stderr below is the real thing,
    /// measured: rustup packs the whole causal chain into a single `error:` line.
    const RUSTUP_DNS_FAILURE: &str = "info: syncing channel updates for 1.95.0-x86_64-unknown-linux-gnu\nerror: could not download file from 'https://nonexistent.invalid.example/dist/channel-rust-1.95.0.toml.sha256' to '/home/user/.rustup/tmp/ug1gpzxowfvbc2w9_file': error downloading file: error sending request for url (https://nonexistent.invalid.example/dist/channel-rust-1.95.0.toml.sha256): client error (Connect): dns error: error resolving DNS: failed to lookup address information: Name or service not known";

    #[test]
    fn a_dns_failure_while_installing_a_toolchain_is_infra() {
        let (v, d) = classify(&outcome(Step::InstallToolchain, "", RUSTUP_DNS_FAILURE)).unwrap();
        assert_eq!(v, Verdict::InfraFail);
        let summary = d.summary.unwrap();
        assert!(
            summary.starts_with("error: could not download file"),
            "an INFRA_FAIL ticket should still say something: {summary}"
        );
        // The `info:` progress line precedes the error, so last-line or first-line-of-stderr would
        // both be wrong here.
        assert!(!summary.contains("syncing channel updates"), "{summary}");
        // rustup's causal tail — "dns error: ..." — sits past the 200-character budget and is cut.
        // That is accepted rather than worked around: the verdict already says INFRA_FAIL, which is
        // the attribution a responder needs, and the full stderr is in the job log.
        assert!(!summary.contains("dns error"), "{summary}");
    }

    /// The containment check for the timeout rule, and the reason it is decided before anything
    /// reads the output. A `cargo build` killed mid-flight may already have emitted a perfectly
    /// real-looking diagnostic — here an `E0308` — but it was killed, so the build never finished
    /// and nothing about the published graph was established. Reading that diagnostic would page
    /// about a break that was never shown to exist.
    #[test]
    fn a_killed_command_is_infra_even_when_it_emitted_a_diagnostic() {
        let mut o = outcome(Step::Build, COMPILE_ERROR_JSON, "");
        assert_eq!(
            classify(&o).unwrap().0,
            Verdict::CompileFail,
            "the same output, not killed, is a compile failure"
        );

        o.timed_out = true;
        let (verdict, d) = classify(&o).unwrap();
        assert_eq!(verdict, Verdict::InfraFail);
        assert!(d.codes.is_empty(), "no codes: the build did not finish");
        assert!(
            d.summary.unwrap().contains("ran past the leg's budget"),
            "and it must say the leg reached no verdict"
        );
    }

    /// Real output, captured by pointing the probe through a dead proxy
    /// (`HTTPS_PROXY=http://127.0.0.1:1`), which is the only way the connector failure was
    /// reproducible. An unreachable network must not stop the window or blame the release.
    #[test]
    fn a_connector_failure_in_the_run_phase_is_infra_not_runtime() {
        let stdout = "running 1 test\n\
             test platform_native_cert_store_completes_a_real_tls_handshake ... FAILED\n\
             \n\
             ---- platform_native_cert_store_completes_a_real_tls_handshake stdout ----\n\
             thread 'platform_native_cert_store_completes_a_real_tls_handshake' (28336) panicked at tests/platform_tls.rs:89:9:\n\
             CANARY_CONNECTOR_FAIL: no response reached us, so no TLS handshake is demonstrated. \
             This is the connector -- the network, or the platform's native certificate store -- \
             and not the service. class=DispatchFailure, err=DispatchFailure(..)\n\
             \n\
             test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out\n";
        let (v, d) = classify(&outcome(Step::Test, stdout, "")).unwrap();
        assert_eq!(
            v,
            Verdict::InfraFail,
            "an unreachable connector is the runner, not the published graph"
        );
        let s = d.summary.unwrap();
        assert!(s.contains("no response reached us"), "{s}");
        assert!(
            !s.contains("panicked at"),
            "the reason, not the file position: {s}"
        );
    }

    /// The containment check, and the one that matters most. The marker buys safety from network
    /// blips by giving up a hard verdict, so it must not generalise: an ordinary assertion failure
    /// in the same run, or anywhere near it, is still a `RUNTIME_FAIL`.
    #[test]
    fn the_connector_marker_does_not_swallow_a_real_assertion_failure() {
        let stdout = "running 2 tests\n\
             test s3_request_response_round_trip ... FAILED\n\
             \n\
             ---- s3_request_response_round_trip stdout ----\n\
             thread 's3_request_response_round_trip' (91) panicked at tests/hermetic.rs:80:5:\n\
             assertion failed: unexpected signature scheme: AWS3-HMAC-SHA1\n\
             \n\
             test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out\n";
        let (v, d) = classify(&outcome(Step::Test, stdout, "")).unwrap();
        assert_eq!(v, Verdict::RuntimeFail);
        assert!(
            d.summary.unwrap().contains("unexpected signature scheme"),
            "and it still names what broke"
        );
    }

    /// Induced for real by making one assertion fail. The point of naming the assertions
    /// individually is that a page can say which capability broke, and cargo's stderr for this is
    /// only `error: test failed, to rerun pass --test hermetic`. The harness output below is the
    /// real thing, trimmed of the passing lines.
    #[test]
    fn a_runtime_failure_summary_names_the_assertion_and_why() {
        let stdout = "running 11 tests\n\
             test deliberately_broken_for_induction ... FAILED\n\
             test s3_request_response_round_trip ... ok\n\
             \n\
             failures:\n\
             \n\
             ---- deliberately_broken_for_induction stdout ----\n\
             thread 'deliberately_broken_for_induction' (14364) panicked at tests/hermetic.rs:995:5:\n\
             assertion `left == right` failed: a published crate changed arithmetic\n  \
               left: 4\n \
              right: 5\n\
             \n\
             test result: FAILED. 10 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out\n";
        let (v, d) = classify(&outcome(
            Step::Test,
            stdout,
            "error: test failed, to rerun pass `--test hermetic`",
        ))
        .unwrap();
        assert_eq!(v, Verdict::RuntimeFail);
        assert_eq!(
            d.summary.as_deref(),
            Some(
                "deliberately_broken_for_induction: assertion `left == right` failed: \
                 a published crate changed arithmetic"
            )
        );
    }

    /// A panic with no assertion message still has to name the test, or the page is unactionable.
    #[test]
    fn a_runtime_panic_without_an_assertion_still_names_the_test() {
        let stdout = "running 1 test\n\
             test credential_refresh_honours_expiry ... FAILED\n\
             \n\
             failures:\n\
             \n\
             ---- credential_refresh_honours_expiry stdout ----\n\
             thread 'credential_refresh_honours_expiry' (91) panicked at tests/hermetic.rs:120:14:\n\
             called `Result::unwrap()` on an `Err` value: ConstructionFailure\n\
             \n\
             test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out\n";
        let (_, d) = classify(&outcome(Step::Test, stdout, "")).unwrap();
        let s = d.summary.unwrap();
        assert_eq!(
            s,
            "credential_refresh_honours_expiry: called `Result::unwrap()` on an `Err` value: \
             ConstructionFailure",
            "the message, not the thread name and file position"
        );
    }

    /// Two assertions failing is one page, and it should name both.
    #[test]
    fn a_runtime_failure_names_every_failing_assertion() {
        let stdout = "running 2 tests\n\
             test presigned_url_is_structurally_fixed_and_byte_stable ... FAILED\n\
             test paginator_follows_the_continuation_token_across_two_pages ... FAILED\n\
             \n\
             test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out\n";
        let (_, d) = classify(&outcome(Step::Test, stdout, "")).unwrap();
        let s = d.summary.unwrap();
        assert!(
            s.contains("presigned_url_is_structurally_fixed_and_byte_stable"),
            "{s}"
        );
        assert!(
            s.contains("paginator_follows_the_continuation_token_across_two_pages"),
            "{s}"
        );
    }

    /// Induced for real with `aws-config = "999"`, which is where the last-line rule was caught:
    /// cargo's final line is a `help:` suggestion, so the page named a version to use instead of
    /// naming the failure.
    #[test]
    fn a_resolve_failure_summary_names_the_error_not_cargos_suggestion() {
        let stderr = "    Updating crates.io index\n\
                      error: failed to select a version for the requirement `aws-config = \"^999\"`\n\
                      candidate versions found which didn't match: 1.13.1, 1.13.0, 1.12.0, ...\n\
                      location searched: crates.io index\n\
                      required by package `canary-build-check v0.1.0 (/tmp/induce-resolve)`\n\
                      help: if you are looking for the prerelease package it needs to be specified explicitly\n    \
                      aws-config = { version = \"0.0.26-alpha\" }";
        let (v, d) = classify(&outcome(Step::GenerateLockfile, "", stderr)).unwrap();
        assert_eq!(v, Verdict::ResolveFail);
        assert_eq!(
            d.summary.as_deref(),
            Some("error: failed to select a version for the requirement `aws-config = \"^999\"`")
        );
    }

    /// The heart of the rule. A build that fails with no diagnostic is not attributable, and
    /// `--offline` is what makes that inference sound: the step could not have been a network
    /// failure, so it was the runner.
    #[test]
    fn a_build_that_fails_without_a_diagnostic_is_infra_not_compile() {
        let (v, _) = classify(&outcome(
            Step::Build,
            r#"{"reason":"build-finished","success":false}"#,
            "error: could not compile `clsfy`\nKilled",
        ))
        .unwrap();
        assert_eq!(v, Verdict::InfraFail, "no diagnostic means no attribution");
    }

    /// `failure-note` is a `compiler-message` but not an error. Counting it would turn a
    /// warnings-only build into a COMPILE_FAIL.
    #[test]
    fn failure_notes_and_warnings_are_not_errors() {
        let json = r#"
{"reason":"compiler-message","target":{"name":"x"},"message":{"level":"warning","code":{"code":"unused_imports"},"message":"unused import"}}
{"reason":"compiler-message","target":{"name":"x"},"message":{"level":"failure-note","code":null,"message":"For more information"}}
{"reason":"build-finished","success":false}
"#;
        let (v, _) = classify(&outcome(Step::Build, json, "")).unwrap();
        assert_eq!(v, Verdict::InfraFail);
    }

    /// A killed process has no exit code. That is infra by construction, and must not panic.
    #[test]
    fn a_signal_killed_step_is_infra() {
        let mut o = outcome(Step::Test, "", "");
        o.exit_code = None;
        let (v, _) = classify(&o).unwrap();
        assert_eq!(v, Verdict::InfraFail);
    }

    /// A test run that dies before the harness reports is the runner, not an assertion.
    #[test]
    fn a_test_run_with_no_harness_output_is_infra() {
        let (v, _) = classify(&outcome(
            Step::Test,
            "",
            "error: test failed, to rerun pass `--test hermetic`\nNo space left on device",
        ))
        .unwrap();
        assert_eq!(v, Verdict::InfraFail);
    }

    /// Multiple distinct codes are all reported, deduplicated, in order — `ssstar` fails with an
    /// E0432 and two E0283s, and a responder wants to see both kinds.
    #[test]
    fn distinct_error_codes_are_collected_in_order_without_duplicates() {
        let json = r#"
{"reason":"compiler-message","target":{"name":"ssstar"},"message":{"level":"error","code":{"code":"E0432"},"message":"unresolved import"}}
{"reason":"compiler-message","target":{"name":"ssstar"},"message":{"level":"error","code":{"code":"E0283"},"message":"type annotations needed"}}
{"reason":"compiler-message","target":{"name":"ssstar"},"message":{"level":"error","code":{"code":"E0283"},"message":"type annotations needed"}}
"#;
        let (v, d) = classify(&outcome(Step::Build, json, "")).unwrap();
        assert_eq!(v, Verdict::CompileFail);
        assert_eq!(d.codes, ["E0432", "E0283"]);
    }

    /// Only the first three steps can reach the network, which is the premise the whole rule rests
    /// on. If a later step ever became non-offline, the discriminator would stop being sound.
    #[test]
    fn only_the_fetching_steps_can_reach_the_network() {
        for s in [Step::GenerateLockfile, Step::InstallToolchain, Step::Fetch] {
            assert!(s.can_reach_the_network());
        }
        for s in [Step::Build, Step::TestNoRun, Step::Test] {
            assert!(!s.can_reach_the_network(), "{s:?} runs --offline");
        }
    }

    /// Only `INFRA_FAIL` lets the floor tier continue. Every hard verdict stops it.
    #[test]
    fn infra_fail_is_the_only_verdict_that_does_not_stop_the_window() {
        assert!(!Verdict::InfraFail.stops_the_window());
        for v in [
            Verdict::ResolveFail,
            Verdict::CompileFail,
            Verdict::HarnessFail,
            Verdict::RuntimeFail,
        ] {
            assert!(v.stops_the_window(), "{v} should stop the window");
            assert!(v.is_hard());
        }
    }

    #[test]
    fn verdicts_serialize_as_the_names_the_design_uses() {
        assert_eq!(
            serde_json::to_string(&Verdict::CompileFail).unwrap(),
            "\"COMPILE_FAIL\""
        );
        assert_eq!(Verdict::InfraFail.to_string(), "INFRA_FAIL");
    }

    #[test]
    fn a_long_summary_is_truncated_without_splitting_a_character() {
        let long = "é".repeat(500);
        let json = format!(
            r#"{{"reason":"compiler-message","target":{{"name":"x"}},"message":{{"level":"error","code":{{"code":"E0001"}},"message":"{long}"}}}}"#
        );
        let (_, d) = classify(&outcome(Step::Build, &json, "")).unwrap();
        let s = d.summary.unwrap();
        assert!(s.chars().count() <= 201, "got {} chars", s.chars().count());
        assert!(s.ends_with('…'));
    }

    /// Found by running a real powerpc64 leg with no `rustup target add`: nineteen crates failed
    /// with `E0463: can't find crate for core`. Those are real diagnostics at `level: "error"`, so
    /// without this they classify as COMPILE_FAIL and page claiming the published graph is broken.
    #[test]
    fn a_missing_cross_target_is_infra_not_a_compile_failure() {
        let json = r#"
{"reason":"compiler-message","target":{"name":"itoa"},"message":{"level":"error","code":{"code":"E0463"},"message":"can't find crate for `core`"}}
{"reason":"compiler-message","target":{"name":"bytes"},"message":{"level":"error","code":{"code":"E0463"},"message":"can't find crate for `core`"}}
{"reason":"build-finished","success":false}
"#;
        let (v, d) = classify(&outcome(Step::Build, json, "")).unwrap();
        assert_eq!(
            v,
            Verdict::InfraFail,
            "a missing sysroot is the runner, not the graph"
        );
        assert_eq!(d.codes, ["E0463"], "the code still reaches the ticket");
    }

    /// The narrowness matters: E0463 for an ordinary crate is a genuine missing dependency.
    #[test]
    fn a_missing_ordinary_crate_is_still_a_compile_failure() {
        let json = r#"{"reason":"compiler-message","target":{"name":"canary-build-check"},"message":{"level":"error","code":{"code":"E0463"},"message":"can't find crate for `aws_config`"}}"#;
        let (v, _) = classify(&outcome(Step::Build, json, "")).unwrap();
        assert_eq!(v, Verdict::CompileFail);
    }

    /// A mixed sysroot reports E0514 rather than E0463, and means the same thing.
    #[test]
    fn an_incompatible_rustc_version_is_infra() {
        let json = r#"{"reason":"compiler-message","target":{"name":"x"},"message":{"level":"error","code":{"code":"E0514"},"message":"found crate `serde` compiled by an incompatible version of rustc"}}"#;
        let (v, _) = classify(&outcome(Step::Build, json, "")).unwrap();
        assert_eq!(v, Verdict::InfraFail);
    }

    /// One real graph error alongside sysroot noise must still be a graph failure.
    #[test]
    fn a_graph_error_mixed_with_sysroot_noise_is_still_a_compile_failure() {
        let json = r#"
{"reason":"compiler-message","target":{"name":"itoa"},"message":{"level":"error","code":{"code":"E0463"},"message":"can't find crate for `core`"}}
{"reason":"compiler-message","target":{"name":"canary-build-check"},"message":{"level":"error","code":{"code":"E0308"},"message":"mismatched types"}}
"#;
        let (v, _) = classify(&outcome(Step::Build, json, "")).unwrap();
        assert_eq!(v, Verdict::CompileFail);
    }

    /// Garbage on stdout must not panic the classifier. A leg that crashes while classifying emits
    /// nothing, which is the silent failure this whole crate exists to remove.
    #[test]
    fn malformed_json_lines_are_ignored_rather_than_fatal() {
        let (v, _) = classify(&outcome(
            Step::Build,
            "not json\n{\"reason\":\n{}\n{\"reason\":\"compiler-message\"}\n",
            "",
        ))
        .unwrap();
        assert_eq!(v, Verdict::InfraFail);
    }
}
