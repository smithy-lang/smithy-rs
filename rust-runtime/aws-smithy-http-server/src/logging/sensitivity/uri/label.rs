/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! A wrapper around a path [`&str`](str) to allow for sensitivity.

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
/// // Path segment 2 is redacted and a trailing greedy label
/// let uri = Label::new(&path, |x| x == 2, None);
/// println!("{uri}");
/// ```
///
/// [httpLabel trait]: https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httplabel-trait
#[allow(missing_debug_implementations)]
#[derive(Clone)]
pub struct Label<'a, F> {
    path: &'a str,
    label_marker: F,
    greedy_label: Option<GreedyLabel>,
}

/// Marks a segment as a greedy label up until a char offset from the end.
///
/// # Example
///
/// The pattern, `/alpha/beta/{greedy+}/trail`, has segment index 2 and offset from the end of 6.
///
/// ```rust
/// # use aws_smithy_http_server::logging::sensitivity::uri::GreedyLabel;
/// let greedy_label = GreedyLabel::new(2, 6);
/// ```
#[derive(Clone, Debug)]
pub struct GreedyLabel {
    segment_index: usize,
    end_offset: usize,
}

impl GreedyLabel {
    /// Constructs a new [`GreedyLabel`] from a segment index and an offset from the end of the URI.
    pub fn new(segment_index: usize, end_offset: usize) -> Self {
        Self {
            segment_index,
            end_offset,
        }
    }
}

impl<'a, F> Label<'a, F> {
    /// Constructs a new [`Label`].
    pub fn new(path: &'a str, label_marker: F, greedy_label: Option<GreedyLabel>) -> Self {
        Self {
            path,
            label_marker,
            greedy_label,
        }
    }
}

impl<'a, F> Display for Label<'a, F>
where
    F: Fn(usize) -> bool,
{
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        if let Some(greedy_label) = &self.greedy_label {
            let (greedy_start, hit_greedy) = self
                .path
                .split('/')
                .skip(1)
                .take(greedy_label.segment_index + 1)
                .enumerate()
                .fold(Ok((0, false)), |acc, (index, segment)| {
                    if index == greedy_label.segment_index {
                        // Greedy label exists.
                        Ok((acc?.0, true))
                    } else {
                        // Prior to greedy segment, use label_marker and increment `greedy_start`.
                        if (self.label_marker)(index) {
                            write!(f, "/{}", Sensitive(segment))?;
                        } else {
                            write!(f, "/{}", segment)?;
                        }
                        Ok((acc?.0 + segment.len() + 1, false))
                    }
                })?;

            if hit_greedy {
                if let Some(end_index) = self.path.len().checked_sub(greedy_label.end_offset) {
                    if greedy_start < end_index {
                        let greedy_redaction = Sensitive(&self.path[greedy_start + 1..end_index]);
                        let remainder = &self.path[end_index..];
                        write!(f, "/{greedy_redaction}{remainder}")?;
                    } else {
                        write!(f, "{}", &self.path[greedy_start..])?;
                    }
                }
            }
        } else {
            for (index, segment) in self.path.split('/').skip(1).enumerate() {
                if (self.label_marker)(index) {
                    write!(f, "/{}", Sensitive(segment))?;
                } else {
                    write!(f, "/{}", segment)?;
                }
            }
        }

        Ok(())
    }
}

/// A [`MakeFmt`] producing [`Label`].
#[derive(Clone)]
pub struct MakeLabel<F> {
    pub(crate) label_marker: F,
    pub(crate) greedy_label: Option<GreedyLabel>,
}

impl<F> Debug for MakeLabel<F> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        f.debug_struct("MakeLabel")
            .field("greedy_label", &self.greedy_label)
            .finish_non_exhaustive()
    }
}

impl<'a, F> MakeFmt<&'a str> for MakeLabel<F>
where
    F: Clone,
{
    type Target = Label<'a, F>;

    fn make(&self, path: &'a str) -> Self::Target {
        Label::new(path, self.label_marker.clone(), self.greedy_label.clone())
    }
}

#[cfg(test)]
mod tests {
    use http::Uri;

    use crate::logging::sensitivity::uri::{tests::EXAMPLES, GreedyLabel};

    use super::Label;

    #[test]
    fn mark_none() {
        let originals = EXAMPLES.into_iter().map(Uri::from_static);
        for original in originals {
            let expected = original.path().to_string();
            let output = Label::new(&original.path(), |_| false, None).to_string();
            assert_eq!(output, expected, "original = {original}");
        }
    }

    #[cfg(not(feature = "unredacted-logging"))]
    const ALL_EXAMPLES: [&str; 19] = [
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
        "http://a/{redacted}",
    ];

    #[cfg(feature = "unredacted-logging")]
    pub const ALL_EXAMPLES: [&str; 19] = EXAMPLES;

    #[test]
    fn mark_all() {
        let originals = EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = ALL_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = Label::new(&original.path(), |_| true, None).to_string();
            assert_eq!(output, expected.path(), "original = {original}");
        }
    }

    #[cfg(not(feature = "unredacted-logging"))]
    pub const GREEDY_EXAMPLES: [&str; 19] = [
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
        "http://a/",
    ];

    #[cfg(feature = "unredacted-logging")]
    pub const GREEDY_EXAMPLES: [&str; 19] = EXAMPLES;

    #[test]
    fn greedy() {
        let originals = EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = GREEDY_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = Label::new(&original.path(), |_| false, Some(GreedyLabel::new(1, 0))).to_string();
            assert_eq!(output, expected.path(), "original = {original}");
        }
    }

    #[cfg(not(feature = "unredacted-logging"))]
    pub const GREEDY_EXAMPLES_OFFSET: [&str; 19] = [
        "g:h",
        "http://a/b/{redacted}g",
        "http://a/b/{redacted}/",
        "http://a/g",
        "http://g",
        "http://a/b/{redacted}p?y",
        "http://a/b/{redacted}g?y",
        "http://a/b/{redacted}p?q#s",
        "http://a/b/{redacted}g",
        "http://a/b/{redacted}g?y#s",
        "http://a/b/{redacted}x",
        "http://a/b/{redacted}x",
        "http://a/b/{redacted}x?y#s",
        "http://a/b/{redacted}p?q",
        "http://a/b/{redacted}/",
        "http://a/b/{redacted}/",
        "http://a/b/",
        "http://a/b/{redacted}g",
        "http://a/",
    ];

    #[cfg(feature = "unredacted-logging")]
    pub const GREEDY_EXAMPLES_OFFSET: [&str; 19] = EXAMPLES;

    #[test]
    fn greedy_offset() {
        let originals = EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = GREEDY_EXAMPLES_OFFSET.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = Label::new(&original.path(), |_| false, Some(GreedyLabel::new(1, 1))).to_string();
            assert_eq!(output, expected.path(), "original = {original}");
        }
    }
}
