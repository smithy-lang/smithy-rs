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

use serde::Serialize;

/// The five verdicts a leg can report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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
}

impl StepOutcome {
    pub fn succeeded(&self) -> bool {
        self.exit_code == Some(0)
    }
}

/// What a responder needs in order to know where to look, carried on the dimensioned metric and
/// into the ticket.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Diagnosis {
    /// Compiler error codes, in order of appearance: `E0308`, `E0432`, …
    pub codes: Vec<String>,
    /// The first error's rendered message, truncated.
    pub summary: Option<String>,
    /// The cargo target the first error came from, which distinguishes a break in the consumer's
    /// own code from one in a test target.
    pub target: Option<String>,
}

/// Classify a failed step.
///
/// Returns `None` for a step that succeeded, so the caller can keep walking the list.
pub fn classify(outcome: &StepOutcome) -> Option<(Verdict, Diagnosis)> {
    if outcome.succeeded() {
        return None;
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
            if assertions_ran_and_failed(&combined) {
                Verdict::RuntimeFail
            } else {
                Verdict::InfraFail
            }
        }
    };
    Some((verdict, diagnose(outcome)))
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
    if d.summary.is_none() {
        // No compiler diagnostic: give the responder the last meaningful line of stderr instead, so
        // an INFRA_FAIL ticket still says something.
        d.summary = outcome
            .stderr
            .lines()
            .rev()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map(truncate);
    }
    d
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
    /// yields a DNS error and no `compiler-message` at all.
    #[test]
    fn a_dns_failure_while_installing_a_toolchain_is_infra() {
        let (v, d) = classify(&outcome(
            Step::InstallToolchain,
            "",
            "error: could not download file\ndns error: failed to lookup address information",
        ))
        .unwrap();
        assert_eq!(v, Verdict::InfraFail);
        assert!(
            d.summary.unwrap().contains("dns error"),
            "an INFRA_FAIL ticket should still say something"
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
