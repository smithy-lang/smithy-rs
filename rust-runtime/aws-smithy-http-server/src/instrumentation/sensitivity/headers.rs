/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! A wrapper around [`HeaderMap`] to allow for sensitivity.

use std::fmt::{Debug, Display, Error, Formatter};

use crate::http::{header::HeaderName, HeaderMap};

use crate::instrumentation::MakeFmt;

use super::Sensitive;

/// Marks the sensitive data of a header pair.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct HeaderMarker {
    /// Set to `true` to mark the value as sensitive.
    pub value: bool,
    /// Set to `Some(x)` to mark `key[x..]` as sensitive.
    pub key_suffix: Option<usize>,
}

/// A wrapper around [`&HeaderMap`](HeaderMap) which modifies the behavior of [`Debug`]. Specific parts of the
/// [`HeaderMap`] are marked as sensitive using a closure. This accommodates the [httpPrefixHeaders trait] and
/// [httpHeader trait].
///
/// The [`Debug`] implementation will respect the `unredacted-logging` flag.
///
/// # Example
///
/// ```
/// # use aws_smithy_http_server::instrumentation::sensitivity::headers::{SensitiveHeaders, HeaderMarker};
/// # use aws_smithy_http_server::http::header::HeaderMap;
/// # let headers = HeaderMap::new();
/// // Headers with keys equal to "header-name" are sensitive
/// let marker = |key|
///     HeaderMarker {
///         value: key == "header-name",
///         key_suffix: None
///     };
/// let headers = SensitiveHeaders::new(&headers, marker);
/// println!("{headers:?}");
/// ```
///
/// [httpPrefixHeaders trait]: https://smithy.io/2.0/spec/http-bindings.html#httpprefixheaders-trait
/// [httpHeader trait]: https://smithy.io/2.0/spec/http-bindings.html#httpheader-trait
pub struct SensitiveHeaders<'a, F> {
    headers: &'a HeaderMap,
    marker: F,
}

impl<'a, F> SensitiveHeaders<'a, F> {
    /// Constructs a new [`SensitiveHeaders`].
    pub fn new(headers: &'a HeaderMap, marker: F) -> Self {
        Self { headers, marker }
    }
}

/// Concatenates the [`Debug`] of [`&str`](str) and ['Sensitive<&str>`](Sensitive).
struct ThenDebug<'a>(&'a str, Sensitive<&'a str>);

impl Debug for ThenDebug<'_> {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        write!(f, "\"{}{}\"", self.0, self.1)
    }
}

/// Allows for formatting of `Left` or `Right` variants.
enum OrFmt<Left, Right> {
    Left(Left),
    Right(Right),
}

impl<Left, Right> Debug for OrFmt<Left, Right>
where
    Left: Debug,
    Right: Debug,
{
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Left(left) => left.fmt(f),
            Self::Right(right) => right.fmt(f),
        }
    }
}

impl<Left, Right> Display for OrFmt<Left, Right>
where
    Left: Display,
    Right: Display,
{
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        match self {
            Self::Left(left) => left.fmt(f),
            Self::Right(right) => right.fmt(f),
        }
    }
}

impl<'a, F> Debug for SensitiveHeaders<'a, F>
where
    F: Fn(&'a HeaderName) -> HeaderMarker,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        let iter = self.headers.iter().map(|(key, value)| {
            let HeaderMarker {
                value: value_sensitive,
                key_suffix,
            } = (self.marker)(key);

            let key = if let Some(key_suffix) = key_suffix {
                let key_str = key.as_str();
                OrFmt::Left(ThenDebug(&key_str[..key_suffix], Sensitive(&key_str[key_suffix..])))
            } else {
                OrFmt::Right(key)
            };

            let value = if value_sensitive {
                OrFmt::Left(Sensitive(value))
            } else {
                OrFmt::Right(value)
            };

            (key, value)
        });

        f.debug_map().entries(iter).finish()
    }
}

/// A [`MakeFmt`] producing [`SensitiveHeaders`].
#[derive(Clone)]
pub struct MakeHeaders<F>(pub(crate) F);

impl<F> Debug for MakeHeaders<F> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("MakeHeaders").field(&"...").finish()
    }
}

impl<'a, F> MakeFmt<&'a HeaderMap> for MakeHeaders<F>
where
    F: Clone,
{
    type Target = SensitiveHeaders<'a, F>;

    fn make(&self, source: &'a HeaderMap) -> Self::Target {
        SensitiveHeaders::new(source, self.0.clone())
    }
}
#[cfg(test)]
mod tests {
    use http::{header::HeaderName, HeaderMap, HeaderValue};

    use super::*;

    // This is needed because we header maps with "{redacted}" are disallowed.
    struct TestDebugMap([(&'static str, &'static str); 4]);

    impl Debug for TestDebugMap {
        fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
            f.debug_map().entries(self.0).finish()
        }
    }

    const HEADER_MAP: [(&str, &str); 4] = [
        ("name-a", "value-a"),
        ("name-b", "value-b"),
        ("prefix-a-x", "value-c"),
        ("prefix-b-y", "value-d"),
    ];

    fn to_header_map<I>(values: I) -> HeaderMap
    where
        I: IntoIterator<Item = (&'static str, &'static str)>,
    {
        values
            .into_iter()
            .map(|(key, value)| (HeaderName::from_static(key), HeaderValue::from_static(value)))
            .collect()
    }

    #[test]
    fn mark_none() {
        let original: HeaderMap = to_header_map(HEADER_MAP);

        let output = SensitiveHeaders::new(&original, |_| HeaderMarker::default());
        assert_eq!(format!("{output:?}"), format!("{original:?}"));
    }

    #[cfg(not(feature = "unredacted-logging"))]
    const ALL_VALUES_HEADER_MAP: [(&str, &str); 4] = [
        ("name-a", "{redacted}"),
        ("name-b", "{redacted}"),
        ("prefix-a-x", "{redacted}"),
        ("prefix-b-y", "{redacted}"),
    ];
    #[cfg(feature = "unredacted-logging")]
    const ALL_VALUES_HEADER_MAP: [(&str, &str); 4] = HEADER_MAP;

    #[test]
    fn mark_all_values() {
        let original: HeaderMap = to_header_map(HEADER_MAP);
        let expected = TestDebugMap(ALL_VALUES_HEADER_MAP);

        let output = SensitiveHeaders::new(&original, |_| HeaderMarker {
            value: true,
            key_suffix: None,
        });
        assert_eq!(format!("{output:?}"), format!("{expected:?}"));
    }

    #[cfg(not(feature = "unredacted-logging"))]
    const NAME_A_HEADER_MAP: [(&str, &str); 4] = [
        ("name-a", "{redacted}"),
        ("name-b", "value-b"),
        ("prefix-a-x", "value-c"),
        ("prefix-b-y", "value-d"),
    ];
    #[cfg(feature = "unredacted-logging")]
    const NAME_A_HEADER_MAP: [(&str, &str); 4] = HEADER_MAP;

    #[test]
    fn mark_name_a_values() {
        let original: HeaderMap = to_header_map(HEADER_MAP);
        let expected = TestDebugMap(NAME_A_HEADER_MAP);

        let output = SensitiveHeaders::new(&original, |name| HeaderMarker {
            value: name == "name-a",
            key_suffix: None,
        });
        assert_eq!(format!("{output:?}"), format!("{expected:?}"));
    }

    #[cfg(not(feature = "unredacted-logging"))]
    const PREFIX_A_HEADER_MAP: [(&str, &str); 4] = [
        ("name-a", "value-a"),
        ("name-b", "value-b"),
        ("prefix-a{redacted}", "value-c"),
        ("prefix-b-y", "value-d"),
    ];
    #[cfg(feature = "unredacted-logging")]
    const PREFIX_A_HEADER_MAP: [(&str, &str); 4] = HEADER_MAP;

    #[test]
    fn mark_prefix_a_values() {
        let original: HeaderMap = to_header_map(HEADER_MAP);
        let expected = TestDebugMap(PREFIX_A_HEADER_MAP);

        let prefix = "prefix-a";
        let output = SensitiveHeaders::new(&original, |name: &HeaderName| HeaderMarker {
            value: false,
            key_suffix: if name.as_str().starts_with(prefix) {
                Some(prefix.len())
            } else {
                None
            },
        });
        assert_eq!(format!("{output:?}"), format!("{:?}", expected));
    }
}
