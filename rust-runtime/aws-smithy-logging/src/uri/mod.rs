/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
mod path;
mod query;

use std::fmt::{Display, Error, Formatter};

use http::Uri;

pub use path::*;
pub use query::*;

use crate::Sensitive;

enum QueryFilter<Q> {
    All,
    Keyed(Q),
}

/// A wrapper around [`&Uri`](Uri) which modifies the behavior of [`Display`]. Different filters can be
/// applied to mark specific parts of the `Uri`.
///
/// The [`Display`] implementation will respect the `debug-logging` flag.
pub struct SensitiveUri<'a, P, Q> {
    uri: &'a Uri,
    path_filter: Option<P>,
    query_filter: Option<QueryFilter<Q>>,
}

impl<'a> SensitiveUri<'a, fn(usize) -> bool, fn(&str) -> bool> {
    /// Constructs a new [`SensitiveUri`] with no filtering.
    pub fn new(uri: &'a Uri) -> Self {
        Self {
            uri,
            path_filter: None,
            query_filter: None,
        }
    }
}

impl<'a, P, Q> SensitiveUri<'a, P, Q> {
    /// Sets specific path segments as sensitive by supplying a filter over the path index.
    /// The filter takes the form `Fn(usize) -> bool` where `usize` represents the index of the
    /// segment and the `bool` marks that segment as sensitive.
    ///
    /// This accommodates the [httpLabel trait].
    ///
    /// # Example
    ///
    /// ```
    /// # use aws_smithy_logging::SensitiveUri;
    /// # use http::Uri;
    /// # let uri = Uri::from_static("http://a/");
    /// // First path segment is sensitive
    /// let uri = SensitiveUri::new(&uri).path_filter(|x| x != 0);
    /// println!("{uri}");
    /// ```
    ///
    /// [httpLabel trait]: https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httplabel-trait
    pub fn path_filter<F>(self, filter: F) -> SensitiveUri<'a, F, Q> {
        SensitiveUri {
            uri: self.uri,
            path_filter: Some(filter),
            query_filter: self.query_filter,
        }
    }

    /// Sets specific query string values as sensitive by supplying a filter over the query string
    /// keys. The filter takes the form `Fn(&str) -> bool` where `&str` represents the key of the
    /// query string pair and the `bool` marks that value as sensitive.
    ///
    /// This will override the [`query_filter`](SensitiveUri::query_filter).
    ///
    /// This accommodates the [httpQuery trait].
    ///
    /// # Example
    ///
    /// ```
    /// # use aws_smithy_logging::SensitiveUri;
    /// # use http::Uri;
    /// # let uri = Uri::from_static("http://a/");
    /// // Query string pair with key "name" is sensitive
    /// let uri = SensitiveUri::new(&uri).query_key_filter(|x| x != "name");
    /// println!("{uri}");
    /// ```
    ///
    /// [httpQuery trait]: https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httpquery-trait
    pub fn query_key_filter<F>(self, filter: F) -> SensitiveUri<'a, P, F> {
        SensitiveUri {
            uri: self.uri,
            path_filter: self.path_filter,
            query_filter: Some(QueryFilter::Keyed(filter)),
        }
    }

    /// Sets specific query string values as sensitive by supplying a filter over the query string
    /// keys. The filter takes the form `Fn(&str) -> bool` where `&str` represents the key of the
    /// query string pair and the `bool` marks that value as sensitive.
    ///
    /// This will override the [`query_key_filter`](SensitiveUri::query_key_filter).
    ///
    /// This accommodates the [httpQueryParams trait].
    ///
    /// # Example
    ///
    /// ```
    /// # use aws_smithy_logging::SensitiveUri;
    /// # use http::Uri;
    /// # let uri = Uri::from_static("http://a/");
    /// // All of the query string is sensitive
    /// let uri = SensitiveUri::new(&uri).query_filter();
    /// println!("{uri}");
    /// ```
    ///
    /// [httpQueryParams trait]: https://awslabs.github.io/smithy/1.0/spec/core/http-traits.html#httpqueryparams-trait
    pub fn query_filter(self) -> SensitiveUri<'a, P, Q> {
        SensitiveUri {
            uri: self.uri,
            path_filter: self.path_filter,
            query_filter: Some(QueryFilter::All),
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
        if let Some(path_filter) = &self.path_filter {
            let path = SensitivePath::new(path).filter(path_filter);
            write!(f, "{path}")?;
        } else {
            write!(f, "{path}")?;
        }

        if let Some(query) = self.uri.query() {
            match &self.query_filter {
                Some(QueryFilter::All) => {
                    let query = Sensitive(query);
                    write!(f, "?{query}")
                }
                Some(QueryFilter::Keyed(query_filter)) => {
                    let query = SensitiveQuery::new(query).filter(query_filter);
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
    fn path_filter_none() {
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
    fn path_filter_first_segment() {
        let originals = EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = FIRST_PATH_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = SensitiveUri::new(&original)
                .path_filter(|x| x != 0)
                .to_string();
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
    fn path_filter_last_segment() {
        let originals = EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = LAST_PATH_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let path_len = original.path().split('/').skip(1).count();
            let output = SensitiveUri::new(&original)
                .path_filter(|x| x + 1 != path_len)
                .to_string();
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
    fn query_filter_all() {
        let originals = QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = ALL_QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = SensitiveUri::new(&original)
                .query_key_filter(|_| false)
                .to_string();
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
    fn query_filter_x() {
        let originals = QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = X_QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = SensitiveUri::new(&original)
                .query_key_filter(|key| key != "x")
                .to_string();
            assert_eq!(output, expected.to_string(), "original = {original}");
        }
    }
}
