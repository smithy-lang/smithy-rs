/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use pretty_assertions::assert_str_eq;
use smithy_rs_tool_common::shell::{handle_failure, output_text};
use std::fs;
use test_bin::get_test_bin;

fn run_with_args(args: &[&str]) -> String {
    let mut cmd = get_test_bin("api-linter");
    for &arg in args {
        cmd.arg(arg);
    }
    let output = cmd.output().expect("failed to start api-linter");
    handle_failure("api-linter", &output).unwrap();
    let (stdout, _) = output_text(&output);
    stdout
}

#[test]
fn with_default_config() {
    let expected_output = fs::read_to_string("tests/default-config-expected-output.txt").unwrap();
    let actual_output = run_with_args(&[
        "--crate-path",
        "test-workspace/test-crate",
        "--target-path",
        "../target",
    ]);
    assert_str_eq!(expected_output, actual_output);
}

#[test]
fn with_some_allowed_types() {
    let expected_output = fs::read_to_string("tests/allow-some-types-expected-output.txt").unwrap();
    let actual_output = run_with_args(&[
        "--crate-path",
        "test-workspace/test-crate",
        "--target-path",
        "../target",
        "--config",
        "tests/allow-some-types.toml",
    ]);
    assert_str_eq!(expected_output, actual_output);
}
