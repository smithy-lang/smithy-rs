/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::fmt::{Debug, Display, Error, Formatter};

use crate::logging::{sensitivity::Sensitive, MakeFmt};

/// A wrapper around a path [`&str`](str) which modifies the behavior of [`Display`]. The suffix of the path is marked
/// as sensitive by providing a byte position. This accommodates the [httpLabel trait with greedy labels].
///
/// The [`Display`] implementation will respect the `unredacted-logging` flag.
///
/// # Example
///
/// ```
/// # use aws_smithy_http_server::logging::sensitivity::uri::GreedyLabel;
/// # use http::Uri;
/// # let uri = "";
/// // Everything after the 3rd byte is redacted
/// let uri = GreedyLabel::new(&uri, 3);
/// println!("{uri}");
/// ```
///
/// [httpLabel trait with greedy labels]: https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#greedy-labels
#[allow(missing_debug_implementations)]
pub struct GreedyLabel<'a> {
    path: &'a str,
    position: usize,
}

impl<'a> GreedyLabel<'a> {
    /// Constructs a new [`GreedyLabel`].
    pub fn new(path: &'a str, position: usize) -> Self {
        Self { path, position }
    }
}

impl<'a> Display for GreedyLabel<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        if self.path.len() < self.position {
            Display::fmt(self.path, f)?;
        } else {
            write!(f, "{}", &self.path[..self.position])?;
            write!(f, "{}", Sensitive(&self.path[self.position..]))?;
        }

        Ok(())
    }
}

/// A [`MakeFmt`] producing [`GreedyLabel`].
#[derive(Debug, Clone)]
pub struct MakeGreedyLabel(pub(crate) usize);

impl<'a> MakeFmt<&'a str> for MakeGreedyLabel {
    type Target = GreedyLabel<'a>;

    fn make(&self, path: &'a str) -> Self::Target {
        GreedyLabel::new(path, self.0)
    }
}

#[cfg(test)]
mod tests {
    use http::Uri;

    use crate::logging::sensitivity::uri::tests::EXAMPLES;

    use super::*;

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
            let output = GreedyLabel::new(&original.path(), 3).to_string();
            assert_eq!(output, expected.path(), "original = {original}");
        }
    }
}
