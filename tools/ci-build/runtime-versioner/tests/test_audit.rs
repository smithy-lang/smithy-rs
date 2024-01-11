/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use test_common::{TestBase, VersionerOutput};

fn run_audit(test_base: &TestBase, index_name: &str, expect_failure: bool) -> VersionerOutput {
    test_base.run_versioner(
        &[
            "audit",
            "--no-fetch",
            "--fake-crates-io-index",
            test_base.test_data.join(index_name).as_str(),
        ],
        expect_failure,
    )
}

/// Test that the audit passes when all the runtime crates are at the
/// special `0.0.0-smithy-rs-head` version, indicating not to use
/// independent crate versions.
#[test]
fn all_smithy_rs_head() {
    let test_base = TestBase::new("all_smithy_rs_head");
    let result = run_audit(&test_base, "base_crates_io_index.toml", false);
    assert!(result.stdout.contains("SUCCESS"));
}

/// Changing an independently versioned runtime crate and version bumping
/// it to a version that's never been published before succeeds.
#[test]
fn change_crate_with_bump() {
    let test_base = TestBase::new("change_crate_with_bump");
    let result = run_audit(&test_base, "base_crates_io_index.toml", false);
    assert!(result.stdout.contains("SUCCESS"));
}

/// Changing an independently versioned runtime crate and version bumping
/// it to a version that's been published before (oops!) fails the audit.
#[test]
fn change_crate_with_bump_to_already_published_version() {
    let test_base = TestBase::new("change_crate_with_bump");
    let result = run_audit(&test_base, "already_published_version.toml", true);
    assert!(result.stderr.contains(
        "aws-smithy-async was changed and version bumped, \
        but the new version number (1.0.1) has already been \
        published to crates.io",
    ));
}

/// Changing an independent runtime crate without version bumping it fails the audit.
#[test]
fn change_crate_without_bump() {
    let test_base = TestBase::new("change_crate_without_bump");
    let result = run_audit(&test_base, "base_crates_io_index.toml", true);
    assert!(result
        .stderr
        .contains("aws-smithy-async changed since release-2023-10-02 and requires a version bump"));
}

/// Adding a new crate that's never been published before passes audit.
#[test]
fn add_new_crate() {
    let test_base = TestBase::new("add_new_crate");
    let result = run_audit(&test_base, "base_crates_io_index.toml", false);
    assert!(result.stderr.contains("'aws-smithy-newcrate' is a new crate (or wasn't independently versioned before) and will publish at 1.0.0"));
    assert!(result.stdout.contains("SUCCESS"));
}

/// Removing an old crate that's been published before passes audit.
#[test]
fn remove_old_crate() {
    let test_base = TestBase::new("remove_old_crate");
    let result = run_audit(&test_base, "base_crates_io_index.toml", false);
    assert!(result
        .stderr
        .contains("runtime crate 'aws-smithy-http' was removed and will not be published"));
    assert!(result.stdout.contains("SUCCESS"));
}
