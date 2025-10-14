/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Wrappers around [`Uri`] and it's constituents to allow for sensitivity.

mod label;
mod query;

use std::fmt::{Debug, Display, Error, Formatter};

use http;

use http::Uri;

pub use label::*;
pub use query::*;

use crate::instrumentation::{MakeDisplay, MakeFmt, MakeIdentity};

/// A wrapper around [`&Uri`](Uri) which modifies the behavior of [`Display`]. Specific parts of the [`Uri`] as are
/// marked as sensitive using the methods provided.
///
/// The [`Display`] implementation will respect the `unredacted-logging` flag.
#[allow(missing_debug_implementations)]
pub struct SensitiveUri<'a, P, Q> {
    uri: &'a Uri,
    make_path: P,
    make_query: Q,
}

impl<'a> SensitiveUri<'a, MakeIdentity, MakeIdentity> {
    /// Constructs a new [`SensitiveUri`] with nothing marked as sensitive.
    pub fn new(uri: &'a Uri) -> Self {
        Self {
            uri,
            make_path: MakeIdentity,
            make_query: MakeIdentity,
        }
    }
}

impl<'a, P, Q> SensitiveUri<'a, P, Q> {
    pub(crate) fn make_path<M>(self, make_path: M) -> SensitiveUri<'a, M, Q> {
        SensitiveUri {
            uri: self.uri,
            make_path,
            make_query: self.make_query,
        }
    }

    pub(crate) fn make_query<M>(self, make_query: M) -> SensitiveUri<'a, P, M> {
        SensitiveUri {
            uri: self.uri,
            make_path: self.make_path,
            make_query,
        }
    }

    /// Marks path segments as sensitive by providing predicate over the segment index.
    ///
    /// See [`Label`] for more info.
    pub fn label<F>(self, label_marker: F, greedy_label: Option<GreedyLabel>) -> SensitiveUri<'a, MakeLabel<F>, Q> {
        self.make_path(MakeLabel {
            label_marker,
            greedy_label,
        })
    }

    /// Marks specific query string values as sensitive by supplying a predicate over the query string keys.
    ///
    /// See [`Query`] for more info.
    pub fn query<F>(self, marker: F) -> SensitiveUri<'a, P, MakeQuery<F>> {
        self.make_query(MakeQuery(marker))
    }
}

impl<'a, P, Q> Display for SensitiveUri<'a, P, Q>
where
    P: MakeDisplay<&'a str>,
    Q: MakeDisplay<&'a str>,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        if let Some(scheme) = self.uri.scheme() {
            write!(f, "{scheme}://")?;
        }

        if let Some(authority) = self.uri.authority() {
            write!(f, "{authority}")?;
        }

        let path = self.uri.path();
        let path = self.make_path.make_display(path);
        write!(f, "{path}")?;

        if let Some(query) = self.uri.query() {
            let query = self.make_query.make_display(query);
            write!(f, "?{query}")?;
        }

        Ok(())
    }
}

/// A [`MakeFmt`] producing [`SensitiveUri`].
#[derive(Clone)]
pub struct MakeUri<P, Q> {
    pub(crate) make_path: P,
    pub(crate) make_query: Q,
}

impl<P, Q> Debug for MakeUri<P, Q> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        f.debug_struct("MakeUri").finish_non_exhaustive()
    }
}

impl<'a, P, Q> MakeFmt<&'a http::Uri> for MakeUri<P, Q>
where
    Q: Clone,
    P: Clone,
{
    type Target = SensitiveUri<'a, P, Q>;

    fn make(&self, source: &'a http::Uri) -> Self::Target {
        SensitiveUri::new(source)
            .make_query(self.make_query.clone())
            .make_path(self.make_path.clone())
    }
}

impl Default for MakeUri<MakeIdentity, MakeIdentity> {
    fn default() -> Self {
        Self {
            make_path: MakeIdentity,
            make_query: MakeIdentity,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::http::Uri;

    use super::{QueryMarker, SensitiveUri};

    // https://www.w3.org/2004/04/uri-rel-test.html
    // NOTE: http::Uri's `Display` implementation trims the fragment, we mirror this behavior
    pub const EXAMPLES: [&str; 19] = [
        "g:h",
        "http://a/b/c/g",
        "http://a/b/c/g/",
        "http://a/g",
        "http://g",
        "http://a/b/c/d;p?y",
        "http://a/b/c/g?y",
        "http://a/b/c/d;p?q#s",
        "http://a/b/c/g#s",
        "http://a/b/c/g?y#s",
        "http://a/b/c/;x",
        "http://a/b/c/g;x",
        "http://a/b/c/g;x?y#s",
        "http://a/b/c/d;p?q",
        "http://a/b/c/",
        "http://a/b/c/",
        "http://a/b/",
        "http://a/b/g",
        "http://a/",
    ];

    pub const QUERY_STRING_EXAMPLES: [&str; 11] = [
        "http://a/b/c/g?&",
        "http://a/b/c/g?x",
        "http://a/b/c/g?x&y",
        "http://a/b/c/g?x&y&",
        "http://a/b/c/g?x&y&z",
        "http://a/b/c/g?x=y&x=z",
        "http://a/b/c/g?x=y&z",
        "http://a/b/c/g?x=y&",
        "http://a/b/c/g?x=y&y=z",
        "http://a/b/c/g?&x=z",
        "http://a/b/c/g?x&x=y",
    ];

    #[test]
    fn path_mark_none() {
        let originals = EXAMPLES.into_iter().map(Uri::from_static);
        for original in originals {
            let output = SensitiveUri::new(&original).to_string();
            assert_eq!(output, original.to_string());
        }
    }

    #[cfg(not(feature = "unredacted-logging"))]
    const FIRST_PATH_EXAMPLES: [&str; 19] = [
        "g:h",
        "http://a/{redacted}/c/g",
        "http://a/{redacted}/c/g/",
        "http://a/{redacted}",
        "http://g/{redacted}",
        "http://a/{redacted}/c/d;p?y",
        "http://a/{redacted}/c/g?y",
        "http://a/{redacted}/c/d;p?q#s",
        "http://a/{redacted}/c/g#s",
        "http://a/{redacted}/c/g?y#s",
        "http://a/{redacted}/c/;x",
        "http://a/{redacted}/c/g;x",
        "http://a/{redacted}/c/g;x?y#s",
        "http://a/{redacted}/c/d;p?q",
        "http://a/{redacted}/c/",
        "http://a/{redacted}/c/",
        "http://a/{redacted}/",
        "http://a/{redacted}/g",
        "http://a/{redacted}",
    ];
    #[cfg(feature = "unredacted-logging")]
    const FIRST_PATH_EXAMPLES: [&str; 19] = EXAMPLES;

    #[test]
    fn path_mark_first_segment() {
        let originals = EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = FIRST_PATH_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = SensitiveUri::new(&original).label(|x| x == 0, None).to_string();
            assert_eq!(output, expected.to_string(), "original = {original}");
        }
    }

    #[cfg(not(feature = "unredacted-logging"))]
    const LAST_PATH_EXAMPLES: [&str; 19] = [
        "g:h",
        "http://a/b/c/{redacted}",
        "http://a/b/c/g/{redacted}",
        "http://a/{redacted}",
        "http://g/{redacted}",
        "http://a/b/c/{redacted}?y",
        "http://a/b/c/{redacted}?y",
        "http://a/b/c/{redacted}?q#s",
        "http://a/b/c/{redacted}#s",
        "http://a/b/c/{redacted}?y#s",
        "http://a/b/c/{redacted}",
        "http://a/b/c/{redacted}",
        "http://a/b/c/{redacted}?y#s",
        "http://a/b/c/{redacted}?q",
        "http://a/b/c/{redacted}",
        "http://a/b/c/{redacted}",
        "http://a/b/{redacted}",
        "http://a/b/{redacted}",
        "http://a/{redacted}",
    ];
    #[cfg(feature = "unredacted-logging")]
    const LAST_PATH_EXAMPLES: [&str; 19] = EXAMPLES;

    #[test]
    fn path_mark_last_segment() {
        let originals = EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = LAST_PATH_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let path_len = original.path().split('/').skip(1).count();
            let output = SensitiveUri::new(&original)
                .label(|x| x + 1 == path_len, None)
                .to_string();
            assert_eq!(output, expected.to_string(), "original = {original}");
        }
    }

    #[cfg(not(feature = "unredacted-logging"))]
    pub const ALL_KEYS_QUERY_STRING_EXAMPLES: [&str; 11] = [
        "http://a/b/c/g?&",
        "http://a/b/c/g?x",
        "http://a/b/c/g?x&y",
        "http://a/b/c/g?x&y&",
        "http://a/b/c/g?x&y&z",
        "http://a/b/c/g?{redacted}=y&{redacted}=z",
        "http://a/b/c/g?{redacted}=y&z",
        "http://a/b/c/g?{redacted}=y&",
        "http://a/b/c/g?{redacted}=y&{redacted}=z",
        "http://a/b/c/g?&{redacted}=z",
        "http://a/b/c/g?x&{redacted}=y",
    ];
    #[cfg(feature = "unredacted-logging")]
    pub const ALL_KEYS_QUERY_STRING_EXAMPLES: [&str; 11] = QUERY_STRING_EXAMPLES;

    #[test]
    fn query_mark_all_keys() {
        let originals = QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = ALL_KEYS_QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = SensitiveUri::new(&original)
                .query(|_| QueryMarker {
                    key: true,
                    value: false,
                })
                .to_string();
            assert_eq!(output, expected.to_string(), "original = {original}");
        }
    }

    #[cfg(not(feature = "unredacted-logging"))]
    pub const ALL_VALUES_QUERY_STRING_EXAMPLES: [&str; 11] = [
        "http://a/b/c/g?&",
        "http://a/b/c/g?x",
        "http://a/b/c/g?x&y",
        "http://a/b/c/g?x&y&",
        "http://a/b/c/g?x&y&z",
        "http://a/b/c/g?x={redacted}&x={redacted}",
        "http://a/b/c/g?x={redacted}&z",
        "http://a/b/c/g?x={redacted}&",
        "http://a/b/c/g?x={redacted}&y={redacted}",
        "http://a/b/c/g?&x={redacted}",
        "http://a/b/c/g?x&x={redacted}",
    ];
    #[cfg(feature = "unredacted-logging")]
    pub const ALL_VALUES_QUERY_STRING_EXAMPLES: [&str; 11] = QUERY_STRING_EXAMPLES;

    #[test]
    fn query_mark_all_values() {
        let originals = QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = ALL_VALUES_QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = SensitiveUri::new(&original)
                .query(|_| QueryMarker {
                    key: false,
                    value: true,
                })
                .to_string();
            assert_eq!(output, expected.to_string(), "original = {original}");
        }
    }

    #[cfg(not(feature = "unredacted-logging"))]
    pub const ALL_PAIRS_QUERY_STRING_EXAMPLES: [&str; 11] = [
        "http://a/b/c/g?&",
        "http://a/b/c/g?x",
        "http://a/b/c/g?x&y",
        "http://a/b/c/g?x&y&",
        "http://a/b/c/g?x&y&z",
        "http://a/b/c/g?{redacted}={redacted}&{redacted}={redacted}",
        "http://a/b/c/g?{redacted}={redacted}&z",
        "http://a/b/c/g?{redacted}={redacted}&",
        "http://a/b/c/g?{redacted}={redacted}&{redacted}={redacted}",
        "http://a/b/c/g?&{redacted}={redacted}",
        "http://a/b/c/g?x&{redacted}={redacted}",
    ];
    #[cfg(feature = "unredacted-logging")]
    pub const ALL_PAIRS_QUERY_STRING_EXAMPLES: [&str; 11] = QUERY_STRING_EXAMPLES;

    #[test]
    fn query_mark_all_pairs() {
        let originals = QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = ALL_PAIRS_QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = SensitiveUri::new(&original)
                .query(|_| QueryMarker { key: true, value: true })
                .to_string();
            assert_eq!(output, expected.to_string(), "original = {original}");
        }
    }

    #[cfg(not(feature = "unredacted-logging"))]
    pub const X_QUERY_STRING_EXAMPLES: [&str; 11] = [
        "http://a/b/c/g?&",
        "http://a/b/c/g?x",
        "http://a/b/c/g?x&y",
        "http://a/b/c/g?x&y&",
        "http://a/b/c/g?x&y&z",
        "http://a/b/c/g?x={redacted}&x={redacted}",
        "http://a/b/c/g?x={redacted}&z",
        "http://a/b/c/g?x={redacted}&",
        "http://a/b/c/g?x={redacted}&y=z",
        "http://a/b/c/g?&x={redacted}",
        "http://a/b/c/g?x&x={redacted}",
    ];
    #[cfg(feature = "unredacted-logging")]
    pub const X_QUERY_STRING_EXAMPLES: [&str; 11] = QUERY_STRING_EXAMPLES;

    #[test]
    fn query_mark_x() {
        let originals = QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = X_QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = SensitiveUri::new(&original)
                .query(|key| QueryMarker {
                    key: false,
                    value: key == "x",
                })
                .to_string();
            assert_eq!(output, expected.to_string(), "original = {original}");
        }
    }
}
