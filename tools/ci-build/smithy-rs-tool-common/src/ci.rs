/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use std::ffi::OsStr;
use std::path::Path;

/// Returns `true` if this code is being run in CI
pub fn running_in_ci() -> bool {
    std::env::var("GITHUB_ACTIONS").unwrap_or_default() == "true"
        || std::env::var("SMITHY_RS_DOCKER_BUILD_IMAGE").unwrap_or_default() == "1"
}

/// The `BUILD_TYPE` env var is only set for Codebuild jobs, will always
/// return `false` in other CI environments
pub fn is_preview_build() -> bool {
    let build_type = std::env::var("BUILD_TYPE");

    if let Ok(build_type) = build_type {
        if build_type.eq_ignore_ascii_case("PREVIEW") {
            return true;
        }
    }

    false
}

pub fn is_in_example_dir(manifest_path: impl AsRef<Path>) -> bool {
    let mut path = manifest_path.as_ref();
    // Check if current dir is examples
    if path.ends_with("examples") {
        return true;
    }
    // Examine parent directories until either `examples/` or `aws-sdk-rust/` is found
    while let Some(parent) = path.parent() {
        path = parent;
        if path.file_name() == Some(OsStr::new("examples")) {
            return true;
        } else if path.file_name() == Some(OsStr::new("aws-sdk-rust")) {
            break;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_in_example_dir() {
        assert!(!is_in_example_dir("aws-sdk-rust/sdk/s3/Cargo.toml"));
        assert!(!is_in_example_dir("aws-sdk-rust/sdk/aws-config/Cargo.toml"));
        assert!(!is_in_example_dir(
            "/path/to/aws-sdk-rust/sdk/aws-config/Cargo.toml"
        ));
        assert!(!is_in_example_dir("sdk/aws-config/Cargo.toml"));
        assert!(is_in_example_dir("examples/foo/Cargo.toml"));
        assert!(is_in_example_dir("examples/foo/bar/Cargo.toml"));
        assert!(is_in_example_dir(
            "aws-sdk-rust/examples/foo/bar/Cargo.toml"
        ));
        assert!(is_in_example_dir("aws-sdk-rust/examples/"));
    }
}
