/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::box_error::BoxError;
use crate::client::interceptors::context::Error;
use aws_smithy_http::body::SdkBody;
use http as http0;
use std::borrow::Cow;
use std::fmt::{Debug, Formatter};
use std::str::FromStr;

/// An HTTP Request Type
pub struct Request<B = SdkBody> {
    inner: http0::Request<B>,
}

impl<B: Debug> Debug for Request<B> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.inner)
    }
}

impl<B> Request<B> {
    /// Converts this request into an http 0.x request.
    ///
    /// Depending on the internal storage type, this operation may be free or it may have an internal
    /// cost.
    pub fn into_http0x(self) -> http::Request<B> {
        self.inner
    }

    /// Returns a reference to the header map
    pub fn headers(&self) -> Headers<'_> {
        Headers::from(&self.inner)
    }

    /// Returns a mutable reference to the header map
    pub fn headers_mut(&mut self) -> HeadersMut<'_> {
        HeadersMut::from(&mut self.inner)
    }

    /// Returns the body associated with the request
    pub fn body(&self) -> &B {
        self.inner.body()
    }
}

impl Request<SdkBody> {
    /// Attempts to clone this request
    ///
    /// If the body is cloneable, this will clone the request. Otherwise `None` will be returned
    pub fn try_clone(&self) -> Option<Self> {
        let body = self.body().try_clone()?;
        let mut cloned_request = http0::Request::builder()
            .uri(self.inner.uri().clone())
            .method(self.inner.method().clone());
        *cloned_request
            .headers_mut()
            .expect("builder has not been modified, headers must be valid") =
            self.inner.headers().clone();
        Some(Self {
            inner: cloned_request
                .body(body)
                .expect("valid->valid must still be valid"),
        })
    }
}

impl<B> TryFrom<http0::Request<B>> for Request<B> {
    type Error = Error;

    fn try_from(value: http::Request<B>) -> Result<Self, Self::Error> {
        if let Some(e) = value
            .headers()
            .values()
            .filter_map(|value| value.to_str().err())
            .next()
        {
            Err(Error::erase(e))
        } else {
            Ok(Self { inner: value })
        }
    }
}

/// An immutable view of request headers
#[derive(Clone)]
pub struct Headers<'a> {
    inner: &'a http::HeaderMap,
}

impl<'a, B> From<&'a http0::Request<B>> for Headers<'a> {
    fn from(value: &'a http0::Request<B>) -> Self {
        Self {
            inner: value.headers(),
        }
    }
}

impl Headers<'_> {
    /// Returns the value for a given key
    ///
    /// If multiple values are associated, the first value is returned
    /// See [http::HeaderMap::get]
    pub fn get(&self, key: impl AsRef<str>) -> Option<&str> {
        self.inner
            .get(key.as_ref())
            .map(|v| v.to_str().expect("we prohibit non-UTF8 header values"))
    }

    /// Returns the total number of **values** stored in the map
    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

/// A mutable view of request headers
pub struct HeadersMut<'a> {
    headers: &'a mut http0::HeaderMap,
}

mod header_value {
    use super::http0;
    use crate::box_error::BoxError;

    /// HeaderValue type
    ///
    /// **Note**: Unlike `HeaderValue` in `http`, this only supports ASCII header values
    pub struct HeaderValue {
        _private: http0::HeaderValue,
    }

    impl HeaderValue {
        pub(crate) fn from_http0(value: http0::HeaderValue) -> Result<Self, BoxError> {
            let _ = value.to_str()?;
            Ok(Self { _private: value })
        }

        pub(crate) fn into_http0(self) -> http0::HeaderValue {
            self._private
        }
    }
}

pub use header_value::HeaderValue;

impl FromStr for HeaderValue {
    type Err = BoxError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !s.is_ascii() {
            Err("header contained non-ascii data")?
        }
        Ok(s.parse()?)
    }
}

impl TryFrom<http0::HeaderValue> for HeaderValue {
    type Error = BoxError;

    fn try_from(value: http::HeaderValue) -> Result<Self, Self::Error> {
        HeaderValue::from_http0(value)
    }
}

impl TryFrom<String> for HeaderValue {
    type Error = BoxError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        HeaderValue::from_http0(http0::HeaderValue::try_from(value)?)
    }
}

trait HeaderName {
    type Out<S>;

    fn into_name(self) -> Self::Out<http0::HeaderName>;
}

impl HeadersMut<'_> {
    /// Insert a value into the headers structure.
    ///
    /// This will *replace* any existing value for this key. Returns the previous associated value if any.
    ///
    /// # Panics
    /// If the key or value are not valid ascii, this function will panic.
    pub fn insert(
        &mut self,
        key: impl Into<MaybeStatic>,
        value: impl Into<MaybeStatic>,
    ) -> Option<String> {
        self.try_insert(key, value)
            .expect("HeaderName or HeadValue was invalid")
    }

    /// Insert a value into the headers structure.
    ///
    /// This will *replace* any existing value for this key. Returns the previous associated value if any.
    ///
    /// If the key or value are not valid ascii, an error is returned
    pub fn try_insert(
        &mut self,
        key: impl Into<MaybeStatic>,
        value: impl Into<MaybeStatic>,
    ) -> Result<Option<String>, BoxError> {
        let key = key.into();
        let value = value.into();
        let key = match key {
            Cow::Borrowed(staticc) => http0::HeaderName::from_static(staticc),
            Cow::Owned(s) => http0::HeaderName::try_from(s)?,
        };
        let value = match value {
            Cow::Borrowed(b) => http0::HeaderValue::from_static(b),
            Cow::Owned(s) => http0::HeaderValue::try_from(s)?,
        };
        let value: HeaderValue = value.try_into()?;
        Ok(self
            .headers
            .insert(key, value.into_http0())
            .map(|old_value| {
                String::from_utf8(old_value.as_bytes().to_vec())
                    .expect("only string values may be inserted")
            }))
    }
}

type MaybeStatic = Cow<'static, str>;

impl<'a, B> From<&'a mut http0::Request<B>> for HeadersMut<'a> {
    fn from(value: &'a mut http::Request<B>) -> Self {
        HeadersMut {
            headers: value.headers_mut(),
        }
    }
}
