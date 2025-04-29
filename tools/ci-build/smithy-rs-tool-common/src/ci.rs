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

pub fn is_preview_build() -> bool {
    let build_type = std::env::var("BUILD_TYPE");

    if let Ok(build_type) = build_type {
        if build_type.eq_ignore_ascii_case("PREVIEW")
            || build_type.eq_ignore_ascii_case("\"PREVIEW\"")
        {
            return true;
        }
    }

    false
}

pub fn is_example_manifest(manifest_path: impl AsRef<Path>) -> bool {
    // Examine parent directories until either `examples/` or `aws-sdk-rust/` is found
    let mut path = manifest_path.as_ref();
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
    fn test_is_example_manifest() {
        assert!(!is_example_manifest("aws-sdk-rust/sdk/s3/Cargo.toml"));
        assert!(!is_example_manifest(
            "aws-sdk-rust/sdk/aws-config/Cargo.toml"
        ));
        assert!(!is_example_manifest(
            "/path/to/aws-sdk-rust/sdk/aws-config/Cargo.toml"
        ));
        assert!(!is_example_manifest("sdk/aws-config/Cargo.toml"));
        assert!(is_example_manifest("examples/foo/Cargo.toml"));
        assert!(is_example_manifest("examples/foo/bar/Cargo.toml"));
        assert!(is_example_manifest(
            "aws-sdk-rust/examples/foo/bar/Cargo.toml"
        ));
    }
}
