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

/// The `main` branch is byte-identical to the `release-2023-10-02` tag, so these tests
/// start from a state where the direct-content audit has nothing to report and construct
/// their version topology in the working tree.
fn dependency_test_base() -> TestBase {
    TestBase::new("main")
}

/// Reproduces the incident this check exists to catch: a dependency moves to an
/// incompatible version line, and the already-published dependent keeps requiring the old
/// line even though nothing in the dependent's own directory changed.
#[test]
fn incompatible_dependency_update_requires_dependent_bump() {
    let test_base = dependency_test_base();
    // `aws-smithy-json` is at `0.0.0-smithy-rs-head` in the fixture. Give it an
    // independent, unpublished version on the `0.61.x` line, which the requirement
    // published for `aws-config 1.0.0` (`^0.60.0`) does not accept.
    test_base.set_crate_version("rust-runtime/aws-smithy-json", "0.61.0");

    let result = run_audit(&test_base, "dependent_requirements.toml", true);
    assert!(result.stderr.contains(
        "aws-config 1.0.0 has already been published, but its published dependency \
         requirements are stale"
    ));
    assert!(result.stderr.contains(
        "normal dependency aws-smithy-json ^0.60.0 does not accept the current version 0.61.0"
    ));
    assert!(result
        .stderr
        .contains("Choose a new, unpublished aws-config version"));
    // The dependency bump itself is fine: `0.61.0` has never been published.
    assert!(!result.stderr.contains("aws-smithy-json was changed"));
    // Nothing under `aws-config/` changed, so the direct-content audit stays quiet.
    assert!(!result.stderr.contains("aws-config changed since"));
}

/// Giving the dependent a new, unpublished version resolves the finding: the publisher
/// stamps the dependency's current version into the manifest before publishing it.
#[test]
fn bumping_the_dependent_resolves_a_stale_requirement() {
    let test_base = dependency_test_base();
    test_base.set_crate_version("rust-runtime/aws-smithy-json", "0.61.0");
    test_base.set_crate_version("aws/rust-runtime/aws-config", "1.0.1");

    let result = run_audit(&test_base, "dependent_requirements.toml", false);
    assert!(result.stdout.contains("SUCCESS"));
}

/// A compatible `0.x` patch update does not force a dependent release.
#[test]
fn compatible_dependency_update_does_not_require_dependent_bump() {
    let test_base = dependency_test_base();
    test_base.set_crate_version("rust-runtime/aws-smithy-json", "0.60.1");

    let result = run_audit(&test_base, "dependent_requirements.toml", false);
    assert!(result.stdout.contains("SUCCESS"));
}

/// For `1.x` crates the minor version is compatible, so no dependent bump is required.
#[test]
fn compatible_one_x_dependency_update_does_not_require_dependent_bump() {
    let test_base = dependency_test_base();
    test_base.set_crate_version("rust-runtime/aws-smithy-async", "1.1.0");

    let result = run_audit(&test_base, "dependent_requirements.toml", false);
    assert!(result.stdout.contains("SUCCESS"));
}

/// A `1.x` major update is incompatible and does require a dependent bump.
#[test]
fn major_one_x_dependency_update_requires_dependent_bump() {
    let test_base = dependency_test_base();
    test_base.set_crate_version("rust-runtime/aws-smithy-async", "2.0.0");

    let result = run_audit(&test_base, "dependent_requirements.toml", true);
    assert!(result.stderr.contains(
        "normal dependency aws-smithy-async ^1.0.0 does not accept the current version 2.0.0"
    ));
}

/// An explicit non-caret requirement is honored exactly: `=1.0.0` rejects `1.0.1`, even
/// though caret semantics would accept it.
#[test]
fn non_caret_requirement_is_honored_exactly() {
    let test_base = dependency_test_base();
    test_base.set_crate_version("rust-runtime/aws-smithy-async", "1.0.1");

    let result = run_audit(&test_base, "dependent_requirements_exact.toml", true);
    assert!(result.stderr.contains(
        "normal dependency aws-smithy-async =1.0.0 does not accept the current version 1.0.1"
    ));

    // The same version is accepted by the caret requirement in the other fixture.
    let result = run_audit(&test_base, "dependent_requirements.toml", false);
    assert!(result.stdout.contains("SUCCESS"));
}
