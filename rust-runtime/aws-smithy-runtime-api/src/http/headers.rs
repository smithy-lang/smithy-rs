/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Types for HTTP headers

use crate::http::error::{HttpError, NonUtf8Header};
use std::borrow::Cow;
use std::fmt::Debug;
use std::str::FromStr;

/// An immutable view of headers
#[derive(Clone, Default, Debug)]
pub struct Headers {
    pub(super) headers: http_02x::HeaderMap<HeaderValue>,
}

impl<'a> IntoIterator for &'a Headers {
    type Item = (&'a str, &'a str);
    type IntoIter = HeadersIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        HeadersIter {
            inner: self.headers.iter(),
        }
    }
}

/// An Iterator over headers
pub struct HeadersIter<'a> {
    inner: http_02x::header::Iter<'a, HeaderValue>,
}

impl<'a> Iterator for HeadersIter<'a> {
    type Item = (&'a str, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(k, v)| (k.as_str(), v.as_ref()))
    }
}

impl Headers {
    /// Create an empty header map
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(feature = "http-1x")]
    pub(crate) fn http1_headermap(self) -> http_1x::HeaderMap {
        let mut headers = http_1x::HeaderMap::new();
        headers.reserve(self.headers.len());
        headers.extend(self.headers.into_iter().map(|(k, v)| {
            (
                k.map(|n| {
                    http_1x::HeaderName::from_bytes(n.as_str().as_bytes()).expect("proven valid")
                }),
                v.into_http1x(),
            )
        }));
        headers
    }

    #[cfg(feature = "http-02x")]
    pub(crate) fn http0_headermap(self) -> http_02x::HeaderMap {
        let mut headers = http_02x::HeaderMap::new();
        headers.reserve(self.headers.len());
        headers.extend(self.headers.into_iter().map(|(k, v)| (k, v.into_http02x())));
        headers
    }

    /// Returns the value for a given key
    ///
    /// If multiple values are associated, the first value is returned
    /// See [HeaderMap::get](http_02x::HeaderMap::get)
    pub fn get(&self, key: impl AsRef<str>) -> Option<&str> {
        self.headers.get(key.as_ref()).map(|v| v.as_ref())
    }

    /// Returns all values for a given key
    pub fn get_all(&self, key: impl AsRef<str>) -> impl Iterator<Item = &str> {
        self.headers
            .get_all(key.as_ref())
            .iter()
            .map(|v| v.as_ref())
    }

    /// Returns an iterator over the headers
    pub fn iter(&self) -> HeadersIter<'_> {
        HeadersIter {
            inner: self.headers.iter(),
        }
    }

    /// Returns the total number of **values** stored in the map
    pub fn len(&self) -> usize {
        self.headers.len()
    }

    /// Returns true if there are no headers
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns true if this header is present
    pub fn contains_key(&self, key: impl AsRef<str>) -> bool {
        self.headers.contains_key(key.as_ref())
    }

    /// Insert a value into the headers structure.
    ///
    /// This will *replace* any existing value for this key. Returns the previous associated value if any.
    ///
    /// # Panics
    /// If the key is not valid ASCII, or if the value is not valid UTF-8, this function will panic.
    pub fn insert(
        &mut self,
        key: impl AsHeaderComponent,
        value: impl AsHeaderComponent,
    ) -> Option<String> {
        let key = header_name(key, false).unwrap();
        let value = header_value(value.into_maybe_static().unwrap(), false).unwrap();
        self.headers
            .insert(key, value)
            .map(|old_value| old_value.into())
    }

    /// Insert a value into the headers structure.
    ///
    /// This will *replace* any existing value for this key. Returns the previous associated value if any.
    ///
    /// If the key is not valid ASCII, or if the value is not valid UTF-8, this function will return an error.
    pub fn try_insert(
        &mut self,
        key: impl AsHeaderComponent,
        value: impl AsHeaderComponent,
    ) -> Result<Option<String>, HttpError> {
        let key = header_name(key, true)?;
        let value = header_value(value.into_maybe_static()?, true)?;
        Ok(self
            .headers
            .insert(key, value)
            .map(|old_value| old_value.into()))
    }

    /// Appends a value to a given key
    ///
    /// # Panics
    /// If the key is not valid ASCII, or if the value is not valid UTF-8, this function will panic.
    pub fn append(&mut self, key: impl AsHeaderComponent, value: impl AsHeaderComponent) -> bool {
        let key = header_name(key.into_maybe_static().unwrap(), false).unwrap();
        let value = header_value(value.into_maybe_static().unwrap(), false).unwrap();
        self.headers.append(key, value)
    }

    /// Appends a value to a given key
    ///
    /// If the key is not valid ASCII, or if the value is not valid UTF-8, this function will return an error.
    pub fn try_append(
        &mut self,
        key: impl AsHeaderComponent,
        value: impl AsHeaderComponent,
    ) -> Result<bool, HttpError> {
        let key = header_name(key.into_maybe_static()?, true)?;
        let value = header_value(value.into_maybe_static()?, true)?;
        Ok(self.headers.append(key, value))
    }

    /// Removes all headers with a given key
    ///
    /// If there are multiple entries for this key, the first entry is returned
    pub fn remove(&mut self, key: impl AsRef<str>) -> Option<String> {
        self.headers
            .remove(key.as_ref())
            .map(|h| h.as_str().to_string())
    }
}

#[cfg(feature = "http-02x")]
impl TryFrom<http_02x::HeaderMap> for Headers {
    type Error = HttpError;

    /// This function attempts to parse header bytes as UTF-8. If that fails we
    /// try to parse the header value as an ISO 8859 string. Our strategy for parsing
    /// as 8859 is infallible since it casts each `u8` to a `char` which reinterprets
    /// it as a UTF-8 codepoint. This works for all 256 possible values of `u8` and
    /// leaves us with a UTF-8 `String` (with possibly different bytes than the ones
    /// sent over the wire).
    fn try_from(value: http_02x::HeaderMap) -> Result<Self, Self::Error> {
        let mut string_safe_headers: http_02x::HeaderMap<HeaderValue> = Default::default();
        value.iter().for_each(|(k, v)| {
            let new_value = HeaderValue::from_http02x(v.clone()).unwrap_or(
                HeaderValue::from_str(iso_8859_to_string(v.as_bytes()).as_str())
                    .expect("Header should be either UTF-8 or ISO 8859"),
            );
            string_safe_headers.insert(k, new_value);
        });

        Ok(Headers {
            headers: string_safe_headers,
        })
    }
}

#[cfg(feature = "http-1x")]
impl TryFrom<http_1x::HeaderMap> for Headers {
    type Error = HttpError;

    /// This function attempts to parse header bytes as UTF-8. If that fails we
    /// try to parse the header value as an ISO 8859 string. Our strategy for parsing
    /// as 8859 is infallible since it casts each `u8` to a `char` which reinterprets
    /// it as a UTF-8 codepoint. This works for all 256 possible values of `u8` and
    /// leaves us with a UTF-8 `String` (with possibly different bytes than the ones
    /// sent over the wire).
    fn try_from(value: http_1x::HeaderMap) -> Result<Self, Self::Error> {
        let mut string_safe_headers: http_02x::HeaderMap<HeaderValue> = Default::default();
        value.iter().for_each(|(k, v)| {
            let new_value = HeaderValue::from_http1x(v.clone()).unwrap_or(
                HeaderValue::from_str(iso_8859_to_string(v.as_bytes()).as_str())
                    .expect("Header should be either UTF-8 or ISO 8859"),
            );
            let new_key =
                http_02x::HeaderName::from_bytes(k.as_str().as_bytes()).expect("known valid");

            string_safe_headers.insert(new_key, new_value);
        });

        Ok(Headers {
            headers: string_safe_headers,
        })
    }
}

use sealed::AsHeaderComponent;

mod sealed {
    use super::*;
    /// Trait defining things that may be converted into a header component (name or value)
    pub trait AsHeaderComponent {
        /// If the component can be represented as a Cow<'static, str>, return it
        fn into_maybe_static(self) -> Result<MaybeStatic, HttpError>;

        /// Return a string reference to this header
        fn as_str(&self) -> Result<&str, HttpError>;

        /// If a component is already internally represented as a `http02x::HeaderName`, return it
        fn repr_as_http02x_header_name(self) -> Result<http_02x::HeaderName, Self>
        where
            Self: Sized,
        {
            Err(self)
        }
    }

    impl AsHeaderComponent for &'static str {
        fn into_maybe_static(self) -> Result<MaybeStatic, HttpError> {
            Ok(Cow::Borrowed(self))
        }

        fn as_str(&self) -> Result<&str, HttpError> {
            Ok(self)
        }
    }

    impl AsHeaderComponent for String {
        fn into_maybe_static(self) -> Result<MaybeStatic, HttpError> {
            Ok(Cow::Owned(self))
        }

        fn as_str(&self) -> Result<&str, HttpError> {
            Ok(self)
        }
    }

    impl AsHeaderComponent for Cow<'static, str> {
        fn into_maybe_static(self) -> Result<MaybeStatic, HttpError> {
            Ok(self)
        }

        fn as_str(&self) -> Result<&str, HttpError> {
            Ok(self.as_ref())
        }
    }

    impl AsHeaderComponent for http_02x::HeaderValue {
        fn into_maybe_static(self) -> Result<MaybeStatic, HttpError> {
            Ok(Cow::Owned(
                std::str::from_utf8(self.as_bytes())
                    .map_err(|err| {
                        HttpError::non_utf8_header(NonUtf8Header::new_missing_name(
                            self.as_bytes().to_vec(),
                            err,
                        ))
                    })?
                    .to_string(),
            ))
        }

        fn as_str(&self) -> Result<&str, HttpError> {
            std::str::from_utf8(self.as_bytes()).map_err(|err| {
                HttpError::non_utf8_header(NonUtf8Header::new_missing_name(
                    self.as_bytes().to_vec(),
                    err,
                ))
            })
        }
    }

    impl AsHeaderComponent for http_02x::HeaderName {
        fn into_maybe_static(self) -> Result<MaybeStatic, HttpError> {
            Ok(self.to_string().into())
        }

        fn as_str(&self) -> Result<&str, HttpError> {
            Ok(self.as_ref())
        }

        fn repr_as_http02x_header_name(self) -> Result<http_02x::HeaderName, Self>
        where
            Self: Sized,
        {
            Ok(self)
        }
    }
}

mod header_value {
    use super::*;

    /// HeaderValue type
    ///
    /// **Note**: Unlike `HeaderValue` in `http`, this only supports UTF-8 header values
    #[derive(Debug, Clone)]
    pub struct HeaderValue {
        _private: Inner,
    }

    #[derive(Debug, Clone)]
    enum Inner {
        H0(http_02x::HeaderValue),
        #[allow(dead_code)]
        H1(http_1x::HeaderValue),
    }

    impl HeaderValue {
        #[allow(dead_code)]
        pub(crate) fn from_http02x(value: http_02x::HeaderValue) -> Result<Self, HttpError> {
            let _ = std::str::from_utf8(value.as_bytes()).map_err(|err| {
                HttpError::non_utf8_header(NonUtf8Header::new_missing_name(
                    value.as_bytes().to_vec(),
                    err,
                ))
            })?;
            Ok(Self {
                _private: Inner::H0(value),
            })
        }

        #[allow(dead_code)]
        pub(crate) fn from_http1x(value: http_1x::HeaderValue) -> Result<Self, HttpError> {
            let _ = std::str::from_utf8(value.as_bytes()).map_err(|err| {
                HttpError::non_utf8_header(NonUtf8Header::new_missing_name(
                    value.as_bytes().to_vec(),
                    err,
                ))
            })?;
            Ok(Self {
                _private: Inner::H1(value),
            })
        }

        #[allow(dead_code)]
        pub(crate) fn into_http02x(self) -> http_02x::HeaderValue {
            match self._private {
                Inner::H0(v) => v,
                Inner::H1(v) => http_02x::HeaderValue::from_maybe_shared(v).expect("unreachable"),
            }
        }

        #[allow(dead_code)]
        pub(crate) fn into_http1x(self) -> http_1x::HeaderValue {
            match self._private {
                Inner::H1(v) => v,
                Inner::H0(v) => http_1x::HeaderValue::from_maybe_shared(v).expect("unreachable"),
            }
        }
    }

    impl AsRef<str> for HeaderValue {
        fn as_ref(&self) -> &str {
            let bytes = match &self._private {
                Inner::H0(v) => v.as_bytes(),
                Inner::H1(v) => v.as_bytes(),
            };
            std::str::from_utf8(bytes).expect("unreachable—only strings may be stored")
        }
    }

    impl From<HeaderValue> for String {
        fn from(value: HeaderValue) -> Self {
            value.as_ref().to_string()
        }
    }

    impl HeaderValue {
        /// Returns the string representation of this header value
        pub fn as_str(&self) -> &str {
            self.as_ref()
        }
    }

    impl FromStr for HeaderValue {
        type Err = HttpError;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            HeaderValue::try_from(s.to_string())
        }
    }

    impl TryFrom<String> for HeaderValue {
        type Error = HttpError;

        fn try_from(value: String) -> Result<Self, Self::Error> {
            Ok(HeaderValue::from_http02x(
                http_02x::HeaderValue::try_from(value).map_err(HttpError::invalid_header_value)?,
            )
            .expect("input was a string"))
        }
    }
}

pub use header_value::HeaderValue;

type MaybeStatic = Cow<'static, str>;

fn header_name(
    name: impl AsHeaderComponent,
    panic_safe: bool,
) -> Result<http_02x::HeaderName, HttpError> {
    name.repr_as_http02x_header_name().or_else(|name| {
        name.into_maybe_static().and_then(|mut cow| {
            if cow.chars().any(|c| c.is_ascii_uppercase()) {
                cow = Cow::Owned(cow.to_ascii_uppercase());
            }
            match cow {
                Cow::Borrowed(s) if panic_safe => {
                    http_02x::HeaderName::try_from(s).map_err(HttpError::invalid_header_name)
                }
                Cow::Borrowed(static_s) => Ok(http_02x::HeaderName::from_static(static_s)),
                Cow::Owned(s) => {
                    http_02x::HeaderName::try_from(s).map_err(HttpError::invalid_header_name)
                }
            }
        })
    })
}

fn header_value(value: MaybeStatic, panic_safe: bool) -> Result<HeaderValue, HttpError> {
    let header = match value {
        Cow::Borrowed(b) if panic_safe => {
            http_02x::HeaderValue::try_from(b).map_err(HttpError::invalid_header_value)?
        }
        Cow::Borrowed(b) => http_02x::HeaderValue::from_static(b),
        Cow::Owned(s) => {
            http_02x::HeaderValue::try_from(s).map_err(HttpError::invalid_header_value)?
        }
    };
    HeaderValue::from_http02x(header)
}

/// Interpret each byte as a unicode codepoint and then build a `String` from these codepoints
#[allow(dead_code)]
fn iso_8859_to_string(s: &[u8]) -> String {
    s.iter().map(|&c| c as char).collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_can_be_any_string() {
        let _: HeaderValue = "😹".parse().expect("can be any string");
        let _: HeaderValue = "abcd".parse().expect("can be any string");
        let _ = "a\nb"
            .parse::<HeaderValue>()
            .expect_err("cannot contain control characters");
    }

    #[test]
    fn no_panic_insert_upper_case_header_name() {
        let mut headers = Headers::new();
        headers.insert("I-Have-Upper-Case", "foo");
    }
    #[test]
    fn no_panic_append_upper_case_header_name() {
        let mut headers = Headers::new();
        headers.append("I-Have-Upper-Case", "foo");
    }

    #[test]
    #[should_panic]
    fn panic_insert_invalid_ascii_key() {
        let mut headers = Headers::new();
        headers.insert("💩", "foo");
    }
    #[test]
    #[should_panic]
    fn panic_insert_invalid_header_value() {
        let mut headers = Headers::new();
        headers.insert("foo", "💩");
    }
    #[test]
    #[should_panic]
    fn panic_append_invalid_ascii_key() {
        let mut headers = Headers::new();
        headers.append("💩", "foo");
    }
    #[test]
    #[should_panic]
    fn panic_append_invalid_header_value() {
        let mut headers = Headers::new();
        headers.append("foo", "💩");
    }

    #[test]
    fn no_panic_try_insert_invalid_ascii_key() {
        let mut headers = Headers::new();
        assert!(headers.try_insert("💩", "foo").is_err());
    }
    #[test]
    fn no_panic_try_insert_invalid_header_value() {
        let mut headers = Headers::new();
        assert!(headers
            .try_insert(
                "foo",
                // Valid header value with invalid UTF-8
                http_02x::HeaderValue::from_bytes(&[0xC0, 0x80]).unwrap()
            )
            .is_err());
    }
    #[test]
    fn no_panic_try_append_invalid_ascii_key() {
        let mut headers = Headers::new();
        assert!(headers.try_append("💩", "foo").is_err());
    }
    #[test]
    fn no_panic_try_append_invalid_header_value() {
        let mut headers = Headers::new();
        assert!(headers
            .try_insert(
                "foo",
                // Valid header value with invalid UTF-8
                http_02x::HeaderValue::from_bytes(&[0xC0, 0x80]).unwrap()
            )
            .is_err());
    }

    #[test]
    fn iso_8859_header_parses_correctly_from_http_1x() {
        // Contains the ISO-8859 single byte multiplication symbol 215
        let iso_8859_header_bytes: &[u8] = &[
            105, 110, 108, 105, 110, 101, 59, 32, 102, 105, 108, 101, 110, 97, 109, 101, 61, 115,
            97, 109, 112, 108, 101, 95, 54, 52, 48, 215, 52, 50, 54, 46, 106, 112, 101, 103,
        ];

        // This replaces the ISO-8859 single byte multiplication character with the two byte
        // UTF-8 codepoint `[195, 151]` for the multiplication symbol
        let utf8_header_bytes: &[u8] = &[
            105, 110, 108, 105, 110, 101, 59, 32, 102, 105, 108, 101, 110, 97, 109, 101, 61, 115,
            97, 109, 112, 108, 101, 95, 54, 52, 48, 195, 151, 52, 50, 54, 46, 106, 112, 101, 103,
        ];

        let mut http_1_headers = http_1x::HeaderMap::new();

        let header_value =
            http_1x::HeaderValue::from_bytes(iso_8859_header_bytes).expect("valid header bytes");
        http_1_headers.insert("content-disposition", header_value);

        let aws_headers = Headers::try_from(http_1_headers);

        assert!(aws_headers.is_ok());

        let aws_headers = aws_headers.unwrap();

        let parsed_header_bytes = aws_headers
            .headers
            .get("content-disposition")
            .unwrap()
            .as_str()
            .as_bytes();

        assert_eq!(parsed_header_bytes, utf8_header_bytes);
    }

    #[test]
    fn iso_8859_header_parses_correctly_from_http_02x() {
        // Contains the ISO-8859 single byte multiplication symbol 215
        let iso_8859_header_bytes: &[u8] = &[
            105, 110, 108, 105, 110, 101, 59, 32, 102, 105, 108, 101, 110, 97, 109, 101, 61, 115,
            97, 109, 112, 108, 101, 95, 54, 52, 48, 215, 52, 50, 54, 46, 106, 112, 101, 103,
        ];

        // This replaces the ISO-8859 single byte multiplication character with the two byte
        // UTF-8 codepoint `[195, 151]` for the multiplication symbol
        let utf8_header_bytes: &[u8] = &[
            105, 110, 108, 105, 110, 101, 59, 32, 102, 105, 108, 101, 110, 97, 109, 101, 61, 115,
            97, 109, 112, 108, 101, 95, 54, 52, 48, 195, 151, 52, 50, 54, 46, 106, 112, 101, 103,
        ];

        let mut http_02_headers = http_02x::HeaderMap::new();

        let header_value =
            http_02x::HeaderValue::from_bytes(iso_8859_header_bytes).expect("valid header bytes");
        http_02_headers.insert("content-disposition", header_value);

        let aws_headers = Headers::try_from(http_02_headers);

        assert!(aws_headers.is_ok());

        let aws_headers = aws_headers.unwrap();

        let parsed_header_bytes = aws_headers
            .headers
            .get("content-disposition")
            .unwrap()
            .as_str()
            .as_bytes();

        assert_eq!(parsed_header_bytes, utf8_header_bytes);
    }

    proptest::proptest! {
        #[test]
        fn insert_header_prop_test(input in ".*") {
            let mut headers = Headers::new();
            let _ = headers.try_insert(input.clone(), input);
        }

        #[test]
        fn append_header_prop_test(input in ".*") {
            let mut headers = Headers::new();
            let _ = headers.try_append(input.clone(), input);
        }
    }
}
