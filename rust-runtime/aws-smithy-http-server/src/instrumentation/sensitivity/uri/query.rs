/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! A wrapper around a query string [`&str`](str) to allow for sensitivity.

use std::fmt::{Debug, Display, Error, Formatter};

use crate::instrumentation::{sensitivity::Sensitive, MakeFmt};

/// Marks the sensitive data of a query string pair.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct QueryMarker {
    /// Set to `true` to mark the key as sensitive.
    pub key: bool,
    /// Set to `true` to mark the value as sensitive.
    pub value: bool,
}

/// A wrapper around a query string [`&str`](str) which modifies the behavior of [`Display`]. Specific query string
/// values are marked as sensitive by providing predicate over the keys. This accommodates the [httpQuery trait] and
/// the [httpQueryParams trait].
///
/// The [`Display`] implementation will respect the `unredacted-logging` flag.
///
/// # Example
///
/// ```
/// # use aws_smithy_http_server::instrumentation::sensitivity::uri::{Query, QueryMarker};
/// # let uri = "";
/// // Query string value with key "name" is redacted
/// let uri = Query::new(&uri, |x| QueryMarker { key: false, value: x == "name" } );
/// println!("{uri}");
/// ```
///
/// [httpQuery trait]: https://smithy.io/2.0/spec/http-bindings.html#httpquery-trait
/// [httpQueryParams trait]: https://smithy.io/2.0/spec/http-bindings.html#httpqueryparams-trait
#[allow(missing_debug_implementations)]
pub struct Query<'a, F> {
    query: &'a str,
    marker: F,
}

impl<'a, F> Query<'a, F> {
    /// Constructs a new [`Query`].
    pub fn new(query: &'a str, marker: F) -> Self {
        Self { query, marker }
    }
}

#[inline]
fn write_pair<'a, F>(section: &'a str, marker: F, f: &mut Formatter<'_>) -> Result<(), Error>
where
    F: Fn(&'a str) -> QueryMarker,
{
    if let Some((key, value)) = section.split_once('=') {
        match (marker)(key) {
            QueryMarker { key: true, value: true } => write!(f, "{}={}", Sensitive(key), Sensitive(value)),
            QueryMarker {
                key: true,
                value: false,
            } => write!(f, "{}={value}", Sensitive(key)),
            QueryMarker {
                key: false,
                value: true,
            } => write!(f, "{key}={}", Sensitive(value)),
            QueryMarker {
                key: false,
                value: false,
            } => write!(f, "{key}={value}"),
        }
    } else {
        write!(f, "{section}")
    }
}

impl<'a, F> Display for Query<'a, F>
where
    F: Fn(&'a str) -> QueryMarker,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        let mut it = self.query.split('&');

        if let Some(section) = it.next() {
            write_pair(section, &self.marker, f)?;
        }

        for section in it {
            write!(f, "&")?;
            write_pair(section, &self.marker, f)?;
        }

        Ok(())
    }
}

/// A [`MakeFmt`] producing [`Query`].
#[derive(Clone)]
pub struct MakeQuery<F>(pub(crate) F);

impl<F> Debug for MakeQuery<F> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        f.debug_tuple("MakeQuery").field(&"...").finish()
    }
}
impl<'a, F> MakeFmt<&'a str> for MakeQuery<F>
where
    F: Clone,
{
    type Target = Query<'a, F>;

    fn make(&self, path: &'a str) -> Self::Target {
        Query::new(path, self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use http::Uri;

    use crate::instrumentation::sensitivity::uri::tests::{
        ALL_KEYS_QUERY_STRING_EXAMPLES, ALL_PAIRS_QUERY_STRING_EXAMPLES, ALL_VALUES_QUERY_STRING_EXAMPLES, EXAMPLES,
        QUERY_STRING_EXAMPLES, X_QUERY_STRING_EXAMPLES,
    };

    use super::*;

    #[test]
    fn mark_none() {
        let originals = EXAMPLES.into_iter().chain(QUERY_STRING_EXAMPLES).map(Uri::from_static);
        for original in originals {
            if let Some(query) = original.query() {
                let output = Query::new(query, |_| QueryMarker::default()).to_string();
                assert_eq!(output, query, "original = {original}");
            }
        }
    }

    #[test]
    fn mark_all_keys() {
        let originals = QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = ALL_KEYS_QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = Query::new(original.query().unwrap(), |_| QueryMarker {
                key: true,
                value: false,
            })
            .to_string();
            assert_eq!(output, expected.query().unwrap(), "original = {original}");
        }
    }

    #[test]
    fn mark_all_values() {
        let originals = QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = ALL_VALUES_QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = Query::new(original.query().unwrap(), |_| QueryMarker {
                key: false,
                value: true,
            })
            .to_string();
            assert_eq!(output, expected.query().unwrap(), "original = {original}");
        }
    }

    #[test]
    fn mark_all_pairs() {
        let originals = QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = ALL_PAIRS_QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = Query::new(original.query().unwrap(), |_| QueryMarker { key: true, value: true }).to_string();
            assert_eq!(output, expected.query().unwrap(), "original = {original}");
        }
    }

    #[test]
    fn mark_x() {
        let originals = QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        let expecteds = X_QUERY_STRING_EXAMPLES.into_iter().map(Uri::from_static);
        for (original, expected) in originals.zip(expecteds) {
            let output = Query::new(original.query().unwrap(), |key| QueryMarker {
                key: false,
                value: key == "x",
            })
            .to_string();
            assert_eq!(output, expected.query().unwrap(), "original = {original}");
        }
    }
}
