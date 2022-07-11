/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
mod path;
mod query;

use std::fmt::{self, Display, Error, Formatter};

use http::Uri;

pub use path::*;
pub use query::*;

use super::Sensitive;

enum QueryMarker<Q> {
    All,
    Keyed(Q),
}

impl<Q> fmt::Debug for QueryMarker<Q> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => write!(f, "All"),
            Self::Keyed(_) => f.debug_tuple("Keyed").field(&"...").finish(),
        }
    }
}

/// A wrapper around [`&Uri`](Uri) which modifies the behavior of [`Display`]. Closures are used to mark specific parts
/// of the [`Uri`] as sensitive.
///
/// The [`Display`] implementation will respect the `debug-logging` flag.
pub struct SensitiveUri<'a, P, Q> {
    uri: &'a Uri,
    path_marker: Option<P>,
    query_marker: Option<QueryMarker<Q>>,
}

impl<'a, P, Q> fmt::Debug for SensitiveUri<'a, P, Q> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("SensitiveUri")
            .field("uri", &self.uri)
            .field("query_marker", &self.query_marker)
            .finish_non_exhaustive()
    }
}

impl<'a> SensitiveUri<'a, fn(usize) -> bool, fn(&str) -> bool> {
    /// Constructs a new [`SensitiveUri`] with nothing marked as sensitive.
    pub fn new(uri: &'a Uri) -> Self {
        Self {
            uri,
            path_marker: None,
            query_marker: None,
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
    pub fn path<F>(self, marker: F) -> SensitiveUri<'a, F, Q> {
        SensitiveUri {
            uri: self.uri,
            path_marker: Some(marker),
            query_marker: self.query_marker,
        }
    }

    /// Marks specific query string values as sensitive by supplying a closure over the query string
    /// keys. The closure takes the form `Fn(&str) -> bool` where `&str` represents the key of the
    /// query string pair and the `bool` marks that value as sensitive.
    ///
    /// This will override the [`query`](SensitiveUri::query).
    ///
    /// This accommodates the [httpQuery trait].
    ///
    /// # Example
    ///
    /// ```
    /// # use aws_smithy_http_server::logging::SensitiveUri;
    /// # use http::Uri;
    /// # let uri = Uri::from_static("http://a/");
    /// // Query string pair with key "name" is sensitive
    /// let uri = SensitiveUri::new(&uri).query_key(|x| x == "name");
    /// println!("{uri}");
    /// ```
    ///
    /// [httpQuery trait]: https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httpquery-trait
    pub fn query_key<F>(self, marker: F) -> SensitiveUri<'a, P, F> {
        SensitiveUri {
            uri: self.uri,
            path_marker: self.path_marker,
            query_marker: Some(QueryMarker::Keyed(marker)),
        }
    }

    /// Marks the entire query string as sensitive.
    ///
    /// This will override the [`query_key`](SensitiveUri::query_key).
    ///
    /// This accommodates the [httpQueryParams trait].
    ///
    /// # Example
    ///
    /// ```
    /// # use aws_smithy_http_server::logging::SensitiveUri;
    /// # use http::Uri;
    /// # let uri = Uri::from_static("http://a/");
    /// // All of the query string is sensitive
    /// let uri = SensitiveUri::new(&uri).query();
    /// println!("{uri}");
    /// ```
    ///
    /// [httpQueryParams trait]: https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httpqueryparams-trait
    pub fn query(self) -> SensitiveUri<'a, P, Q> {
        SensitiveUri {
            uri: self.uri,
            path_marker: self.path_marker,
            query_marker: Some(QueryMarker::All),
        }
    }
}

impl<'a, P, Q> Display for SensitiveUri<'a, P, Q>
where
    P: Fn(usize) -> bool,
    Q: Fn(&'a str) -> bool,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        if let Some(scheme) = self.uri.scheme() {
            write!(f, "{scheme}://")?;
        }

        if let Some(authority) = self.uri.authority() {
            write!(f, "{authority}")?;
        }

        let path = self.uri.path();
        if let Some(path_marker) = &self.path_marker {
            let path = SensitivePath::new(path).mark(path_marker);
            write!(f, "{path}")?;
        } else {
            write!(f, "{path}")?;
        }

        if let Some(query) = self.uri.query() {
            match &self.query_marker {
                Some(QueryMarker::All) => {
                    let query = Sensitive(query);
                    write!(f, "?{query}")
                }
                Some(QueryMarker::Keyed(query_marker)) => {
                    let query = SensitiveQuery::new(query).mark(query_marker);
                    write!(f, "?{query}")
                }
                None => write!(f, "?{query}"),
            }?
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use http::Uri;

    use super::SensitiveUri;

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

    #[cfg(not(feature = "debug-logging"))]
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
    #[cfg(feature = "debug-logging")]
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

    #[cfg(not(feature = "debug-logging"))]
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
    #[cfg(feature = "debug-logging")]
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

    #[cfg(not(feature = "debug-logging"))]
    pub const ALL_QUERY_STRING_EXAMPLES: [&str; 11] = [
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
    #[cfg(feature = "debug-logging")]
    pub const ALL_QUERY_STRING_EXAMPLES: [&str; 11] = QUERY_STRING_EXAMPLES;

    #[test]
    fn query_mark_all() {
        let originals = QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = ALL_QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = SensitiveUri::new(&original).query_key(|_| true).to_string();
            assert_eq!(output, expected.to_string(), "original = {original}");
        }
    }

    #[cfg(not(feature = "debug-logging"))]
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
    #[cfg(feature = "debug-logging")]
    pub const X_QUERY_STRING_EXAMPLES: [&str; 11] = QUERY_STRING_EXAMPLES;

    #[test]
    fn query_mark_x() {
        let originals = QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = X_QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = SensitiveUri::new(&original).query_key(|key| key == "x").to_string();
            assert_eq!(output, expected.to_string(), "original = {original}");
        }
    }
}
