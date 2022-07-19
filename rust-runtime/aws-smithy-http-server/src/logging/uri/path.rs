/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use std::fmt::{self, Display, Error, Formatter};

use crate::logging::Sensitive;

pub(crate) fn noop_path_marker(_: usize) -> bool {
    false
}

/// A wrapper around a path [`&str`](str) which modifies the behavior of [`Display`]. Closures are used to mark
/// specific parts of the path as sensitive.
///
/// The [`Display`] implementation will respect the `unredacted-logging` flag.
pub struct SensitivePath<'a, F> {
    path: &'a str,
    marker: F,
}

impl<'a, F> fmt::Debug for SensitivePath<'a, F> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("SensitiveQuery")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl<'a> SensitivePath<'a, fn(usize) -> bool> {
    /// Constructs a new [`SensitivePath`] with nothing marked as sensitive.
    pub fn new(path: &'a str) -> Self {
        Self {
            path,
            marker: noop_path_marker,
        }
    }
}

impl<'a, F> SensitivePath<'a, F> {
    /// Marks specific path segments as sensitive by supplying a closure over the path index.
    ///
    /// See [SensitiveUri::path](super::SensitiveUri::path).
    pub fn mark<G>(self, marker: G) -> SensitivePath<'a, G> {
        SensitivePath {
            path: self.path,
            marker,
        }
    }
}

impl<'a, F> Display for SensitivePath<'a, F>
where
    F: Fn(usize) -> bool,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        for (index, segment) in self.path.split('/').skip(1).enumerate() {
            if (self.marker)(index) {
                write!(f, "/{}", Sensitive(segment))?;
            } else {
                write!(f, "/{}", segment)?;
            }
        }

        Ok(())
    }
}

/// A wrapper around a path [`&str`](str) which modifies the behavior of [`Display`]. A position is used to redact the
/// suffix of the path.
///
/// The [`Display`] implementation will respect the `unredacted-logging` flag.
#[derive(Debug)]
pub struct SensitiveGreedyPath<'a> {
    path: &'a str,
    position: usize,
}

impl<'a> SensitiveGreedyPath<'a> {
    /// Constructs a new [`SensitiveGreedyPath`] with position marking the redacted suffix.
    pub fn new(path: &'a str, position: usize) -> Self {
        Self { path, position }
    }
}

impl<'a> Display for SensitiveGreedyPath<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        if self.path.len() < self.position {
            self.path.fmt(f)?;
        } else {
            write!(f, "{}", &self.path[..self.position])?;
            write!(f, "{}", Sensitive(&self.path[self.position..]))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use http::Uri;

    use crate::logging::uri::tests::EXAMPLES;

    use super::{SensitiveGreedyPath, SensitivePath};

    #[test]
    fn mark_none() {
        let originals = EXAMPLES.into_iter().map(Uri::from_static);
        for original in originals {
            let expected = original.path().to_string();
            let output = SensitivePath::new(&original.path()).to_string();
            assert_eq!(output, expected, "original = {original}");
        }
    }

    #[cfg(not(feature = "unredacted-logging"))]
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

    #[cfg(feature = "unredacted-logging")]
    pub const ALL_EXAMPLES: [&str; 22] = EXAMPLES;

    #[test]
    fn mark_all() {
        let originals = EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = ALL_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = SensitivePath::new(&original.path()).mark(|_| true).to_string();
            assert_eq!(output, expected.path(), "original = {original}");
        }
    }

    #[cfg(not(feature = "unredacted-logging"))]
    pub const GREEDY_EXAMPLES: [&str; 22] = [
        "g:h",
        "http://a/b/{redacted}",
        "http://a/b/{redacted}",
        "http://a/g",
        "http://g",
        "http://a/b/{redacted}?y",
        "http://a/b/{redacted}?y",
        "http://a/b/{redacted}?q#s",
        "http://a/b/{redacted}",
        "http://a/b/{redacted}?y#s",
        "http://a/b/{redacted}",
        "http://a/b/{redacted}",
        "http://a/b/{redacted}?y#s",
        "http://a/b/{redacted}?q",
        "http://a/b/{redacted}",
        "http://a/b/{redacted}",
        "http://a/b/{redacted}",
        "http://a/b/{redacted}",
        "http://a/b/{redacted}",
        "http://a/",
        "http://a/",
        "http://a/g",
    ];

    #[cfg(feature = "unredacted-logging")]
    pub const GREEDY_EXAMPLES: [&str; 22] = EXAMPLES;

    #[test]
    fn greedy() {
        let originals = EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = GREEDY_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = SensitiveGreedyPath::new(&original.path(), 3).to_string();
            assert_eq!(output, expected.path(), "original = {original}");
        }
    }
}
