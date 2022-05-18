/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use pretty_assertions::assert_str_eq;
use smithy_rs_tool_common::shell::{handle_failure, output_text};
use std::fs;
use std::path::Path;
use test_bin::get_test_bin;

fn run_with_args(in_path: impl AsRef<Path>, args: &[&str]) -> String {
    let mut cmd = get_test_bin("cargo-api-linter");
    cmd.current_dir(in_path.as_ref());
    cmd.arg("api-linter");
    for &arg in args {
        cmd.arg(arg);
    }
    let output = cmd.output().expect("failed to start cargo-api-linter");
    match output.status.code() {
        Some(1) => { /* expected */ }
        _ => handle_failure("cargo-api-linter", &output).unwrap(),
    }
    let (stdout, _) = output_text(&output);
    stdout
}

#[test]
fn with_default_config() {
    let expected_output = fs::read_to_string("tests/default-config-expected-output.txt").unwrap();
    let actual_output = run_with_args("test-workspace/test-crate", &[]);
    assert_str_eq!(expected_output, actual_output);
}

#[test]
fn with_some_allowed_types() {
    let expected_output = fs::read_to_string("tests/allow-some-types-expected-output.txt").unwrap();
    let actual_output = run_with_args(
        "test-workspace/test-crate",
        &["--config", "../../tests/allow-some-types.toml"],
    );
    assert_str_eq!(expected_output, actual_output);
}

#[test]
fn with_output_format_markdown_table() {
    let expected_output =
        fs::read_to_string("tests/output-format-markdown-table-expected-output.md").unwrap();
    let actual_output = run_with_args(
        "test-workspace/test-crate",
        &["--output-format", "markdown-table"],
    );
    assert_str_eq!(expected_output, actual_output);
}
