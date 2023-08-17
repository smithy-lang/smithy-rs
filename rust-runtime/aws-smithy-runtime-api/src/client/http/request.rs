/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http::body::SdkBody;
use http as http0;
use http0::header::Iter;
use http0::{Extensions, HeaderMap, Method};
use std::borrow::Cow;
use std::fmt::Debug;
use std::str::{FromStr, Utf8Error};

#[derive(Debug)]
/// An HTTP Request Type
pub struct Request<B = SdkBody> {
    body: B,
    uri: Uri,
    method: Method,
    extensions: Extensions,
    headers: Headers,
}

/// A Request URI
#[derive(Debug, Clone)]
pub struct Uri {
    as_string: String,
    parsed: http0::Uri,
}

impl TryFrom<String> for Uri {
    type Error = HttpError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let parsed = value.parse().map_err(HttpError::invalid_uri)?;
        Ok(Uri {
            as_string: value,
            parsed,
        })
    }
}

impl From<http0::Uri> for Uri {
    fn from(value: http::Uri) -> Self {
        Self {
            as_string: value.to_string(),
            parsed: value,
        }
    }
}

impl<B> TryInto<http0::Request<B>> for Request<B> {
    type Error = HttpError;

    fn try_into(self) -> Result<http::Request<B>, Self::Error> {
        self.into_http03x()
    }
}

impl<B> Request<B> {
    /// Converts this request into an http 0.x request.
    ///
    /// Depending on the internal storage type, this operation may be free or it may have an internal
    /// cost.
    pub fn into_http03x(self) -> Result<http0::Request<B>, HttpError> {
        let mut req = http::Request::builder()
            .uri(self.uri.parsed)
            .method(self.method)
            .body(self.body)
            .expect("known valid");
        let mut headers = HeaderMap::new();
        headers.extend(
            self.headers
                .headers
                .into_iter()
                .map(|(k, v)| (k, v.into_http03x())),
        );
        *req.headers_mut() = headers;
        *req.extensions_mut() = self.extensions;
        Ok(req)
    }

    /// Returns a GET request with no URI
    pub fn new(body: B) -> Self {
        Self {
            body,
            uri: Uri::from(http0::Uri::from_static("/")),
            method: Method::GET,
            extensions: Default::default(),
            headers: Default::default(),
        }
    }

    /// Returns a reference to the header map
    pub fn headers(&self) -> &Headers {
        &self.headers
    }

    /// Returns a mutable reference to the header map
    pub fn headers_mut(&mut self) -> &mut Headers {
        &mut self.headers
    }

    /// Returns the body associated with the request
    pub fn body(&self) -> &B {
        &self.body
    }

    /// Returns a mutable reference to the body
    pub fn body_mut(&mut self) -> &mut B {
        &mut self.body
    }

    /// Returns the method associated with this request
    pub fn method(&self) -> &str {
        self.method.as_str()
    }

    /// Returns the URI associated with this request
    pub fn uri(&self) -> &str {
        &self.uri.as_string
    }

    /// Sets the URI of this request
    pub fn set_uri<U>(&mut self, uri: U) -> Result<(), U::Error>
    where
        U: TryInto<Uri>,
    {
        let uri = uri.try_into()?;
        self.uri = uri;
        Ok(())
    }

    /// Adds an extension to the request extensions
    pub fn add_extension<T: Send + Sync + Clone + 'static>(&mut self, extension: T) {
        self.extensions.insert(extension);
    }
}

impl Request<SdkBody> {
    /// Attempts to clone this request
    ///
    /// If the body is cloneable, this will clone the request. Otherwise `None` will be returned
    pub fn try_clone(&self) -> Option<Self> {
        let body = self.body().try_clone()?;
        Some(Self {
            body,
            uri: self.uri.clone(),
            method: self.method.clone(),
            extensions: Extensions::new(),
            headers: self.headers.clone(),
        })
    }

    /// Replaces this requests body with [`SdkBody::taken()`]
    pub fn take_body(&mut self) -> SdkBody {
        std::mem::replace(self.body_mut(), SdkBody::taken())
    }
}

impl<B> TryFrom<http0::Request<B>> for Request<B> {
    type Error = HttpError;

    fn try_from(value: http::Request<B>) -> Result<Self, Self::Error> {
        if let Some(e) = value
            .headers()
            .values()
            .filter_map(|value| value.to_str().err())
            .next()
        {
            Err(HttpError::header_was_not_a_string(e))
        } else {
            let (parts, body) = value.into_parts();
            let mut string_safe_headers: HeaderMap<HeaderValue> = Default::default();
            string_safe_headers.extend(
                parts
                    .headers
                    .into_iter()
                    .map(|(k, v)| (k, v.try_into().expect("validated above"))),
            );
            Ok(Self {
                body,
                uri: parts.uri.into(),
                method: parts.method.clone(),
                extensions: parts.extensions,
                headers: Headers {
                    headers: string_safe_headers,
                },
            })
        }
    }
}

/// An immutable view of request headers
#[derive(Clone, Default, Debug)]
pub struct Headers {
    headers: HeaderMap<HeaderValue>,
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
    inner: Iter<'a, HeaderValue>,
}

impl<'a> Iterator for HeadersIter<'a> {
    type Item = (&'a str, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(k, v)| (k.as_str(), v.as_ref()))
    }
}

impl Headers {
    /// Returns the value for a given key
    ///
    /// If multiple values are associated, the first value is returned
    /// See [HeaderMap::get]
    pub fn get(&self, key: impl AsRef<str>) -> Option<&str> {
        self.headers.get(key.as_ref()).map(|v| v.as_ref())
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
    pub fn contains_key(&self, key: &str) -> bool {
        self.headers.contains_key(key)
    }

    /// Insert a value into the headers structure.
    ///
    /// This will *replace* any existing value for this key. Returns the previous associated value if any.
    ///
    /// # Panics
    /// If the key or value are not valid ascii, this function will panic.
    pub fn insert(
        &mut self,
        key: impl AsHeaderComponent,
        value: impl AsHeaderComponent,
    ) -> Option<String> {
        self.try_insert(key, value)
            .expect("HeaderName or HeaderValue was invalid")
    }

    /// Insert a value into the headers structure.
    ///
    /// This will *replace* any existing value for this key. Returns the previous associated value if any.
    ///
    /// If the key or value are not valid ascii, an error is returned
    pub fn try_insert(
        &mut self,
        key: impl AsHeaderComponent,
        value: impl AsHeaderComponent,
    ) -> Result<Option<String>, HttpError> {
        let key = header_name(key.into_maybe_static()?)?;
        let value = header_value(value.into_maybe_static()?)?;
        Ok(self
            .headers
            .insert(key, value)
            .map(|old_value| old_value.into()))
    }

    /// Appends a value to a given key
    ///
    /// If the key or value are NOT valid ascii, an error is returned
    pub fn try_append(
        &mut self,
        key: impl AsHeaderComponent,
        value: impl AsHeaderComponent,
    ) -> Result<bool, HttpError> {
        let key = header_name(key.into_maybe_static()?)?;
        let value = header_value(value.into_maybe_static()?)?;
        Ok(self.headers.append(key, value))
    }

    /// Appends a value to a given key
    ///
    /// # Panics
    /// If the key or value are NOT valid ascii, this function will panic
    pub fn append(&mut self, key: impl AsHeaderComponent, value: impl AsHeaderComponent) -> bool {
        self.try_append(key, value)
            .expect("HeaderName or HeaderValue was invalid")
    }
}

use sealed::AsHeaderComponent;

mod sealed {
    use super::*;
    /// Trait defining things that may be converted into a header component (name or value)
    pub trait AsHeaderComponent {
        fn into_maybe_static(self) -> Result<MaybeStatic, HttpError>;

        /// If a component is already internally represented as a `HeaderName`, return it
        fn try_as_http03_header_name(self) -> Result<http0::HeaderName, Self>
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
    }

    impl AsHeaderComponent for String {
        fn into_maybe_static(self) -> Result<MaybeStatic, HttpError> {
            Ok(Cow::Owned(self))
        }
    }

    impl AsHeaderComponent for Cow<'static, str> {
        fn into_maybe_static(self) -> Result<MaybeStatic, HttpError> {
            Ok(self)
        }
    }

    impl AsHeaderComponent for http0::HeaderValue {
        fn into_maybe_static(self) -> Result<MaybeStatic, HttpError> {
            Ok(Cow::Owned(
                self.to_str()
                    .map_err(HttpError::header_was_not_a_string)?
                    .to_string(),
            ))
        }
    }

    impl AsHeaderComponent for http0::HeaderName {
        fn into_maybe_static(self) -> Result<MaybeStatic, HttpError> {
            Ok(self.to_string().into())
        }

        fn try_as_http03_header_name(self) -> Result<http0::HeaderName, Self>
        where
            Self: Sized,
        {
            Ok(self)
        }
    }
}

mod header_value {
    use super::http0;
    use std::str::Utf8Error;

    /// HeaderValue type
    ///
    /// **Note**: Unlike `HeaderValue` in `http`, this only supports UTF-8 header values
    #[derive(Debug, Clone)]
    pub struct HeaderValue {
        _private: http0::HeaderValue,
    }

    impl HeaderValue {
        pub(crate) fn from_http03x(value: http0::HeaderValue) -> Result<Self, Utf8Error> {
            let _ = std::str::from_utf8(value.as_bytes())?;
            Ok(Self { _private: value })
        }

        pub(crate) fn into_http03x(self) -> http0::HeaderValue {
            self._private
        }
    }

    impl AsRef<str> for HeaderValue {
        fn as_ref(&self) -> &str {
            std::str::from_utf8(self._private.as_bytes())
                .expect("unreachable—only strings may be stored")
        }
    }

    impl From<HeaderValue> for String {
        fn from(value: HeaderValue) -> Self {
            value.as_ref().to_string()
        }
    }
}

use crate::client::http::HttpError;
pub use header_value::HeaderValue;

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

impl TryFrom<http0::HeaderValue> for HeaderValue {
    type Error = Utf8Error;

    fn try_from(value: http::HeaderValue) -> Result<Self, Self::Error> {
        HeaderValue::from_http03x(value)
    }
}

impl TryFrom<String> for HeaderValue {
    type Error = HttpError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(HeaderValue::from_http03x(
            http0::HeaderValue::try_from(value).map_err(HttpError::invalid_header_value)?,
        )
        .expect("input was a string"))
    }
}

type MaybeStatic = Cow<'static, str>;

fn header_name(name: impl AsHeaderComponent) -> Result<http0::HeaderName, HttpError> {
    name.try_as_http03_header_name().or_else(|name| {
        name.into_maybe_static().and_then(|cow| match cow {
            Cow::Borrowed(staticc) => Ok(http0::HeaderName::from_static(staticc)),
            Cow::Owned(s) => http0::HeaderName::try_from(s).map_err(HttpError::invalid_header_name),
        })
    })
}

fn header_value(value: MaybeStatic) -> Result<HeaderValue, HttpError> {
    let header = match value {
        Cow::Borrowed(b) => http0::HeaderValue::from_static(b),
        Cow::Owned(s) => {
            http0::HeaderValue::try_from(s).map_err(HttpError::invalid_header_value)?
        }
    };
    header.try_into().map_err(HttpError::new)
}

#[cfg(test)]
mod test {
    use http::HeaderValue;

    #[test]
    fn headers_can_be_any_string() {
        let _: HeaderValue = "😹".parse().expect("can be any string");
        let _: HeaderValue = "abcd".parse().expect("can be any string");
        let _ = "a\nb"
            .parse::<HeaderValue>()
            .expect_err("cannot contain control characters");
    }
}
