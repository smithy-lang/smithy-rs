/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use std::fmt::{Display, Error, Formatter};

use crate::Sensitive;

fn noop_path_filter(_: usize) -> bool {
    true
}

/// A wrapper around a path [`&str`](str) which modifies the behavior of [`Display`]. Different filters can be
/// applied to mark specific parts of the path.
///
/// The [`Display`] implementation will respect the `debug-logging` flag.
pub struct SensitivePath<'a, F> {
    path: &'a str,
    filter: F,
}

impl<'a> SensitivePath<'a, fn(usize) -> bool> {
    /// Constructs a new [`SensitivePath`] with no filtering.
    pub fn new(path: &'a str) -> Self {
        Self {
            path,
            filter: noop_path_filter,
        }
    }
}

impl<'a, F> SensitivePath<'a, F> {
    /// Sets specific path segments as sensitive by supplying a filter over the path index.
    ///
    /// See [SensitiveUri::path_filter](super::SensitiveUri::path_filter).
    pub fn filter<G>(self, filter: G) -> SensitivePath<'a, G> {
        SensitivePath {
            path: self.path,
            filter,
        }
    }
}

impl<'a, F> Display for SensitivePath<'a, F>
where
    F: Fn(usize) -> bool,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        for (index, segment) in self.path.split('/').skip(1).enumerate() {
            if (self.filter)(index) {
                write!(f, "/{}", segment)?;
            } else {
                write!(f, "/{}", Sensitive(segment))?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use http::Uri;

    use crate::uri::tests::EXAMPLES;

    use super::SensitivePath;

    #[test]
    fn filter_none() {
        let originals = EXAMPLES.into_iter().map(Uri::from_static);
        for original in originals {
            let expected = original.path().to_string();
            let output = SensitivePath::new(&original.path()).to_string();
            assert_eq!(output, expected, "original = {original}");
        }
    }

    #[cfg(not(feature = "debug-logging"))]
    const ALL_EXAMPLES: [&str; 22] = [
        "g:h",
        "http://a/{redacted}/{redacted}/{redacted}",
        "http://a/{redacted}/{redacted}/{redacted}/{redacted}",
        "http://a/{redacted}",
        "http://a/{redacted}",
        "http://a/{redacted}/{redacted}/{redacted}",
        "http://a/{redacted}/{redacted}/{redacted}",
        "http://a/{redacted}/{redacted}/{redacted}",
        "http://a/{redacted}/{redacted}/{redacted}",
        "http://a/{redacted}/{redacted}/{redacted}",
        "http://a/{redacted}/{redacted}/{redacted}",
        "http://a/{redacted}/{redacted}/{redacted}",
        "http://a/{redacted}/{redacted}/{redacted}",
        "http://a/{redacted}/{redacted}/{redacted}",
        "http://a/{redacted}/{redacted}/{redacted}",
        "http://a/{redacted}/{redacted}/{redacted}",
        "http://a/{redacted}/{redacted}",
        "http://a/{redacted}/{redacted}",
        "http://a/{redacted}/{redacted}",
        "http://a/{redacted}",
        "http://a/{redacted}",
        "http://a/{redacted}",
    ];

    #[cfg(feature = "debug-logging")]
    pub const ALL_EXAMPLES: [&str; 22] = EXAMPLES;

    #[test]
    fn filter_all() {
        let originals = EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = ALL_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = SensitivePath::new(&original.path())
                .filter(|_| false)
                .to_string();
            assert_eq!(output, expected.path(), "original = {original}");
        }
    }
}
