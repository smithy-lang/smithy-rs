/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
mod path;
mod query;

use std::fmt::{Debug, Display, Error, Formatter};

use http::Uri;

pub use path::*;
pub use query::*;

/// A wrapper around [`&Uri`](Uri) which modifies the behavior of [`Display`]. Closures are used to mark specific parts
/// of the [`Uri`] as sensitive.
///
/// The [`Display`] implementation will respect the `unredacted-logging` flag.
pub struct SensitiveUri<'a, P, Q> {
    uri: &'a Uri,
    path_marker: P,
    query_marker: Q,
}

impl<'a, P, Q> Debug for SensitiveUri<'a, P, Q> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        f.debug_struct("SensitiveUri")
            .field("uri", &self.uri)
            .finish_non_exhaustive()
    }
}

impl<'a> SensitiveUri<'a, Labels<fn(usize) -> bool>, fn(&str) -> QueryMarker> {
    /// Constructs a new [`SensitiveUri`] with nothing marked as sensitive.
    pub fn new(uri: &'a Uri) -> Self {
        Self {
            uri,
            path_marker: Labels(noop_path_marker),
            query_marker: noop_query_marker,
        }
    }
}

impl<'a, P, Q> SensitiveUri<'a, P, Q> {
    /// Marks specific path segments as sensitive by supplying a closure over the path index.
    /// The closure takes the form `Fn(usize) -> bool` where `usize` represents the index of the
    /// segment and the `bool` marks that segment as sensitive.
    ///
    /// This accommodates the [httpLabel trait].
    ///
    /// # Example
    ///
    /// ```
    /// # use aws_smithy_http_server::logging::SensitiveUri;
    /// # use http::Uri;
    /// # let uri = Uri::from_static("http://a/");
    /// // First path segment is sensitive
    /// let uri = SensitiveUri::new(&uri).path(|x| x == 0);
    /// println!("{uri}");
    /// ```
    ///
    /// [httpLabel trait]: https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httplabel-trait
    pub fn path<F>(self, marker: F) -> SensitiveUri<'a, Labels<F>, Q> {
        SensitiveUri {
            uri: self.uri,
            path_marker: Labels(marker),
            query_marker: self.query_marker,
        }
    }

    /// Marks path segments as sensitive by supplying a closure over the path index.
    /// The closure takes the form `Fn(usize) -> bool` where `usize` represents the index of the
    /// segment and the `bool` marks that segment as sensitive.
    ///
    /// This accommodates the [httpLabel trait].
    ///
    /// # Example
    ///
    /// ```
    /// # use aws_smithy_http_server::logging::SensitiveUri;
    /// # use http::Uri;
    /// # let uri = Uri::from_static("http://a/b/c");
    /// // First path segment is sensitive
    /// let uri = SensitiveUri::new(&uri).greedy_path(3);
    /// println!("{uri}");
    /// ```
    ///
    /// [httpLabel trait]: https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httplabel-trait
    pub fn greedy_path(self, position: usize) -> SensitiveUri<'a, GreedyLabel, Q> {
        SensitiveUri {
            uri: self.uri,
            path_marker: GreedyLabel(position),
            query_marker: self.query_marker,
        }
    }

    /// Marks specific query string values as sensitive by supplying a closure over the query string
    /// keys. The closure takes the form `Fn(&str) -> Option<QueryMarker>` where `&str` represents the key of the
    /// query string pair and the `Option<QueryMarker>` marks the key, value, or entire pair as sensitive.
    ///
    /// This accommodates the [httpQuery trait] and [httpQueryParams trait].
    ///
    /// # Example
    ///
    /// ```
    /// # use aws_smithy_http_server::logging::{SensitiveUri, QueryMarker};
    /// # use http::Uri;
    /// # let uri = Uri::from_static("http://a/");
    /// // Query string value with key "name" is sensitive
    /// let uri = SensitiveUri::new(&uri).query(|x| QueryMarker { key: false, value: x == "name" });
    /// println!("{uri}");
    /// ```
    ///
    /// [httpQuery trait]: https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httpquery-trait
    /// [httpQueryParams trait]: https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httpqueryparams-trait
    pub fn query<F>(self, marker: F) -> SensitiveUri<'a, P, F> {
        SensitiveUri {
            uri: self.uri,
            path_marker: self.path_marker,
            query_marker: marker,
        }
    }
}

impl<'a, P, Q> Display for SensitiveUri<'a, P, Q>
where
    P: MakePath<'a>,
    Q: Fn(&'a str) -> QueryMarker,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        if let Some(scheme) = self.uri.scheme() {
            write!(f, "{scheme}://")?;
        }

        if let Some(authority) = self.uri.authority() {
            write!(f, "{authority}")?;
        }

        let path = self.uri.path();
        let path = self.path_marker.make(path);
        write!(f, "{path}")?;

        if let Some(query) = self.uri.query() {
            let query = SensitiveQuery::new(query).mark(&self.query_marker);
            write!(f, "?{query}")?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use http::Uri;

    use super::{QueryMarker, SensitiveUri};

    // https://www.w3.org/2004/04/uri-rel-test.html
    // NOTE: http::Uri's `Display` implementation trims the fragment, we mirror this behavior
    pub const EXAMPLES: [&str; 22] = [
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
        "http://a/b/",
        "http://a/b/g",
        "http://a/",
        "http://a/",
        "http://a/g",
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
    const FIRST_PATH_EXAMPLES: [&str; 22] = [
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
        "http://a/{redacted}/",
        "http://a/{redacted}/g",
        "http://a/{redacted}",
        "http://a/{redacted}",
        "http://a/{redacted}",
    ];
    #[cfg(feature = "unredacted-logging")]
    const FIRST_PATH_EXAMPLES: [&str; 22] = EXAMPLES;

    #[test]
    fn path_mark_first_segment() {
        let originals = EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = FIRST_PATH_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = SensitiveUri::new(&original).path(|x| x == 0).to_string();
            assert_eq!(output, expected.to_string(), "original = {original}");
        }
    }

    #[cfg(not(feature = "unredacted-logging"))]
    const LAST_PATH_EXAMPLES: [&str; 22] = [
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
        "http://a/b/{redacted}",
        "http://a/{redacted}",
        "http://a/{redacted}",
        "http://a/{redacted}",
    ];
    #[cfg(feature = "unredacted-logging")]
    const LAST_PATH_EXAMPLES: [&str; 22] = EXAMPLES;

    #[test]
    fn path_mark_last_segment() {
        let originals = EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = LAST_PATH_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let path_len = original.path().split('/').skip(1).count();
            let output = SensitiveUri::new(&original).path(|x| x + 1 == path_len).to_string();
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
