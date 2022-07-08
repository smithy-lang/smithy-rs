/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use std::fmt::{Display, Error, Formatter};

use crate::Sensitive;

fn noop_query_filter(_: &str) -> bool {
    true
}

/// A wrapper around a query [`&str`](str) which modifies the behavior of [`Display`]. A filter can be applied
/// to filter out specific query parameter values based on their key.
///
/// The [`Display`] implementation will respect the `debug-logging` flag.
pub struct SensitiveQuery<'a, F> {
    query: &'a str,
    filter: F,
}

impl<'a> SensitiveQuery<'a, fn(&str) -> bool> {
    /// Constructs a new [`SensitiveQuery`] with no filtering.
    pub fn new(query: &'a str) -> Self {
        Self {
            query,
            filter: noop_query_filter,
        }
    }
}

impl<'a, F> SensitiveQuery<'a, F> {
    /// Sets specific query string values as sensitive by supplying a filter over the query string
    /// keys. The filter takes the form `Fn(&str) -> bool` where `&str` represents the key of the
    /// query string pair and the `bool` marks that value as sensitive.
    ///
    /// See [SensitiveUri::query_](super::SensitiveUri::path_filter).
    pub fn filter<G>(self, filter: G) -> SensitiveQuery<'a, G> {
        SensitiveQuery {
            query: self.query,
            filter,
        }
    }
}

#[inline]
fn write_key<'a, F>(section: &'a str, filter: F, f: &mut Formatter<'_>) -> Result<(), Error>
where
    F: Fn(&'a str) -> bool,
{
    if let Some((key, value)) = section.split_once('=') {
        if (filter)(key) {
            write!(f, "{key}={value}")
        } else {
            write!(f, "{key}={}", Sensitive(value))
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
            write_key(section, &self.filter, f)?;
        }

        for section in it {
            write!(f, "&")?;
            write_key(section, &self.filter, f)?;
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
    fn filter_none() {
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
    fn filter_all() {
        let originals = QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = ALL_QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = SensitiveQuery::new(&original.query().unwrap())
                .filter(|_| false)
                .to_string();
            assert_eq!(output, expected.query().unwrap(), "original = {original}");
        }
    }

    #[test]
    fn filter_x() {
        let originals = QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = X_QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = SensitiveQuery::new(&original.query().unwrap())
                .filter(|key| key != "x")
                .to_string();
            assert_eq!(output, expected.query().unwrap(), "original = {original}");
        }
    }
}
