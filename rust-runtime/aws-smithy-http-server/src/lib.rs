/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/* Automatically managed default lints */
#![cfg_attr(docsrs, feature(doc_cfg))]
/* End of automatically managed default lints */
#![allow(clippy::derive_partial_eq_without_eq)]

//! HTTP server runtime and utilities, loosely based on [axum].
//!
//! [axum]: https://docs.rs/axum/latest/axum/
#[macro_use]
pub(crate) mod macros;

pub mod body;
pub(crate) mod error;
pub mod extension;
pub mod instrumentation;
pub mod layer;
pub mod operation;
pub mod plugin;
#[doc(hidden)]
pub mod protocol;
#[doc(hidden)]
pub mod rejection;
pub mod request;
#[doc(hidden)]
pub mod response;
pub mod routing;
#[doc(hidden)]
pub mod runtime_error;
pub mod serve;
pub mod service;
pub mod shape_id;

#[doc(inline)]
pub(crate) use self::error::Error;
#[doc(inline)]
pub use self::request::extension::Extension;
#[doc(inline)]
pub use self::serve::serve;
#[doc(inline)]
pub use tower_http::add_extension::{AddExtension, AddExtensionLayer};

#[cfg(test)]
mod test_helpers;

pub use http;

#[cfg(test)]
mod dependency_tests {
    #[test]
    #[ignore]
    fn test_http_body_0_4_only_from_aws_smithy_types() {
        // This test ensures that http-body 0.4 is only brought in by aws-smithy-types
        // and not by any other direct dependencies of aws-smithy-http-server.
        //
        // Run: cargo tree --invert http-body:0.4.6
        // Expected: Only aws-smithy-types should be in the dependency path

        let output = std::process::Command::new("cargo")
            .args(["tree", "--invert", "http-body:0.4.6"])
            .output()
            .expect("Failed to run cargo tree");

        let stdout = String::from_utf8(output.stdout).expect("Invalid UTF-8");

        // Check that aws-smithy-types is the only direct dependency bringing in http-body 0.4
        let lines: Vec<&str> = stdout.lines().collect();

        // Find the line with http-body v0.4.6
        let http_body_line_idx = lines
            .iter()
            .position(|line| line.contains("http-body v0.4.6"))
            .expect("http-body 0.4.6 not found in dependency tree");

        // The next line should show aws-smithy-types as the direct dependent
        let dependent_line = lines
            .get(http_body_line_idx + 1)
            .expect("No dependent found for http-body 0.4.6");

        assert!(
            dependent_line.contains("aws-smithy-types"),
            "http-body 0.4.6 should only be brought in by aws-smithy-types, but found: {}",
            dependent_line
        );
    }
}
