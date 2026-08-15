/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/// Route-prefix configuration for a modeled HTTP route.
#[derive(Debug, Clone)]
pub struct PathPrefix {
    prefixes: &'static [&'static str],
    allow_unprefixed: bool,
}

impl PathPrefix {
    /// Creates an operation-local route-prefix policy.
    pub const fn new(prefixes: &'static [&'static str], allow_unprefixed: bool) -> Self {
        Self {
            prefixes,
            allow_unprefixed,
        }
    }

    #[doc(hidden)]
    pub fn match_path<'a>(&self, path: &'a str) -> Option<&'a str> {
        let without_leading_slash = path.strip_prefix('/').expect("HTTP URI paths must begin with a slash");
        let first_segment = without_leading_slash.split('/').next().unwrap_or_default();

        if self.prefixes.contains(&first_segment) {
            let suffix = &without_leading_slash[first_segment.len()..];
            Some(if suffix.is_empty() { "/" } else { suffix })
        } else if self.allow_unprefixed {
            Some(path)
        } else {
            None
        }
    }

    pub(crate) fn match_uri_path<'a>(&self, uri: &'a http::Uri) -> Option<&'a str> {
        self.match_path(uri.path()).or_else(|| {
            self.log_rejection(uri);
            None
        })
    }

    pub(crate) fn log_rejection(&self, uri: &http::Uri) {
        tracing::debug!(
            path = uri.path(),
            allowed_prefixes = ?self.prefixes,
            "request path does not match this route's prefix policy",
        );
    }
}
