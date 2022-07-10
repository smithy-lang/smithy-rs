/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use std::fmt::{self, Display, Error, Formatter};

use crate::Sensitive;

pub(crate) fn noop_query_marker(_: &str) -> bool {
    false
}

/// A wrapper around a query [`&str`](str) which modifies the behavior of [`Display`]. Closures are used to mark
/// query parameter values as sensitive based on their key.
///
/// The [`Display`] implementation will respect the `debug-logging` flag.
pub struct SensitiveQuery<'a, F> {
    query: &'a str,
    marker: F,
}

impl<'a, F> fmt::Debug for SensitiveQuery<'a, F> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("SensitiveQuery")
            .field("query", &self.query)
            .finish_non_exhaustive()
    }
}

impl<'a> SensitiveQuery<'a, fn(&str) -> bool> {
    /// Constructs a new [`SensitiveQuery`] with nothing marked as sensitive.
    pub fn new(query: &'a str) -> Self {
        Self {
            query,
            marker: noop_query_marker,
        }
    }
}

impl<'a, F> SensitiveQuery<'a, F> {
    /// Marks specific query string values as sensitive by supplying a closure over the query string
    /// keys. The closure takes the form `Fn(&str) -> bool` where `&str` represents the key of the
    /// query string pair and the `bool` marks that value as sensitive.
    ///
    /// See [SensitiveUri::query_key](crate::SensitiveUri::query_key).
    pub fn mark<G>(self, marker: G) -> SensitiveQuery<'a, G> {
        SensitiveQuery {
            query: self.query,
            marker,
        }
    }
}

#[inline]
fn write_key<'a, F>(section: &'a str, marker: F, f: &mut Formatter<'_>) -> Result<(), Error>
where
    F: Fn(&'a str) -> bool,
{
    if let Some((key, value)) = section.split_once('=') {
        if (marker)(key) {
            write!(f, "{key}={}", Sensitive(value))
        } else {
            write!(f, "{key}={value}")
        }
    } else {
        write!(f, "{section}")
    }
}

impl<'a, F> Display for SensitiveQuery<'a, F>
where
    F: Fn(&'a str) -> bool,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        let mut it = self.query.split('&');

        if let Some(section) = it.next() {
            write_key(section, &self.marker, f)?;
        }

        for section in it {
            write!(f, "&")?;
            write_key(section, &self.marker, f)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use http::Uri;

    use crate::uri::tests::{
        ALL_QUERY_STRING_EXAMPLES, EXAMPLES, QUERY_STRING_EXAMPLES, X_QUERY_STRING_EXAMPLES,
    };

    use super::SensitiveQuery;

    #[test]
    fn mark_none() {
        let originals = EXAMPLES
            .into_iter()
            .chain(QUERY_STRING_EXAMPLES)
            .map(Uri::from_static);
        for original in originals {
            if let Some(query) = original.query() {
                let output = SensitiveQuery::new(&query).to_string();
                assert_eq!(output, query, "original = {original}");
            }
        }
    }

    #[test]
    fn mark_all() {
        let originals = QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = ALL_QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = SensitiveQuery::new(&original.query().unwrap())
                .mark(|_| true)
                .to_string();
            assert_eq!(output, expected.query().unwrap(), "original = {original}");
        }
    }

    #[test]
    fn mark_x() {
        let originals = QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = X_QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = SensitiveQuery::new(&original.query().unwrap())
                .mark(|key| key == "x")
                .to_string();
            assert_eq!(output, expected.query().unwrap(), "original = {original}");
        }
    }
}
