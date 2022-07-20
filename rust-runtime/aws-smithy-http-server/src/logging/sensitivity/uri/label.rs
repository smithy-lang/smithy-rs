/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::fmt::{Debug, Display, Error, Formatter};

use crate::logging::{sensitivity::Sensitive, MakeFmt};

/// A wrapper around a path [`&str`](str) which modifies the behavior of [`Display`]. Specific path segments are marked
/// as sensitive by providing predicate over the segment index. This accommodates the [httpLabel trait] with
/// non-greedy labels.
///
/// The [`Display`] implementation will respect the `unredacted-logging` flag.
///
/// # Example
///
/// ```
/// # use aws_smithy_http_server::logging::sensitivity::uri::Label;
/// # use http::Uri;
/// # let path = "";
/// // Path segment 2 is redacted
/// let uri = Label::new(&path, |x| x == 2);
/// println!("{uri}");
/// ```
///
/// [httpLabel trait]: https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httplabel-trait
#[allow(missing_debug_implementations)]
#[derive(Clone)]
pub struct Label<'a, F> {
    path: &'a str,
    marker: F,
}

impl<'a, F> Label<'a, F> {
    /// Constructs a new [`Label`].
    pub fn new(path: &'a str, marker: F) -> Self {
        Self { path, marker }
    }
}

impl<'a, F> Display for Label<'a, F>
where
    F: Fn(usize) -> bool,
{
    #[inline]
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

/// A [`MakeFmt`] producing [`Label`].
#[derive(Clone)]
pub struct MakeLabel<F>(pub(crate) F);

impl<F> Debug for MakeLabel<F> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        f.debug_tuple("MakeLabel").field(&"...").finish()
    }
}

impl<'a, F> MakeFmt<&'a str> for MakeLabel<F>
where
    F: Clone,
{
    type Target = Label<'a, F>;

    fn make(&self, path: &'a str) -> Self::Target {
        Label::new(path, self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use http::Uri;

    use crate::logging::sensitivity::uri::tests::EXAMPLES;

    use super::Label;

    #[test]
    fn mark_none() {
        let originals = EXAMPLES.into_iter().map(Uri::from_static);
        for original in originals {
            let expected = original.path().to_string();
            let output = Label::new(&original.path(), |_| false).to_string();
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
            let output = Label::new(&original.path(), |_| true).to_string();
            assert_eq!(output, expected.path(), "original = {original}");
        }
    }
}
