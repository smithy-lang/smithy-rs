/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Compile-fail tests for code that is not public, run with `ui_test`.
//!
//! Each file in `tests/compile_fail` is compiled on its own by `rustc`, with this crate's
//! dependencies available. A file that tests private code includes that code's source with
//! `#[path]`, so it compiles the real implementation without the crate exposing it. Expected
//! errors are `//~` annotations on the offending line, matched by error code or message, and
//! a file marked `//@check-pass` must compile. There are no `.stderr` snapshots, so compiler
//! upgrades that only reword other parts of a diagnostic do not break these tests.
//!
//! Run them with `cargo test -p aws-smithy-lang --features runtime-schema --test compile_fail`.

use ui_test::custom_flags::rustfix::RustfixMode;
use ui_test::dependencies::DependencyBuilder;
use ui_test::{ignore_output_conflict, run_tests, Config};

fn main() -> ui_test::Result<()> {
    let mut config = Config::rustc("tests/compile_fail");
    // Separate from the target directory this test runs from, which cargo has locked.
    config.out_dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("compile_fail");
    // Only the `//~` annotations are checked, not the full compiler output.
    config.output_conflict_handling = ignore_output_conflict;
    let defaults = config.comment_defaults.base();
    defaults.set_custom("rustfix", RustfixMode::Disabled);
    // Builds this crate's dependencies, so test files can use `aws_smithy_lang` and
    // `aws_smithy_schema`.
    let mut dependencies = DependencyBuilder::default();
    dependencies
        .program
        .args
        .push("--features=runtime-schema".into());
    defaults.set_custom("dependencies", dependencies);
    run_tests(config)
}
