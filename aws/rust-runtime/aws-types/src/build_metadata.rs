/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

include!(concat!(env!("OUT_DIR"), "/build_env.rs"));

struct BuildMetadata {
    rust_version: &'static str,
    core_pkg_version: &'static str
}

static BUILD_METADATA: BuildMetadata = BuildMetadata {
    rust_version: RUST_VERSION,
    core_pkg_version: env!("CARGO_PKG_VERSION")
};

#[cfg(test)]
mod test {
    use crate::build_metadata::BUILD_METADATA;

    #[test]
    fn valid_build_metadata() {
        let meta = &BUILD_METADATA;
        // obviously a slightly brittle test. Will be a small update for Rust 2.0 and GA :-)
        assert!(meta.rust_version.starts_with("1."));
        assert!(meta.core_pkg_version.starts_with("0."));
    }
}
