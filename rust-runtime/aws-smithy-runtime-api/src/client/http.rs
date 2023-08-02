/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::box_error::BoxError;
use aws_smithy_http::body::SdkBody;
use aws_smithy_http::endpoint::error::InvalidEndpointError;
use bytes::Bytes;
use http::header::ToStrError;
use http::{HeaderMap, HeaderName, HeaderValue};
use std::borrow::Cow;
use std::error::Error;

/// An HTTP Request
#[derive(Debug)]
pub struct Request<B = SdkBody> {
    request: http::Request<B>,
}

impl<B> From<http::Request<B>> for Request<B> {
    fn from(value: http::Request<B>) -> Self {
        assert!(
            value.extensions().is_empty(),
            "Cannot be constructed from a request with extensions"
        );
        Self { request: value }
    }
}

impl<B> Request<B> {
    pub fn method(&self) -> &str {
        self.request.method().as_str()
    }

    pub fn into_http0(self) -> http::Request<B> {
        self.request
    }

    pub fn body_mut(&mut self) -> &mut B {
        self.request.body_mut()
    }

    pub fn new(body: B) -> Self {
        Self {
            request: http::Request::new(body),
        }
    }

    pub fn apply_endpoint(&mut self, endpoint: &str) -> Result<(), BoxError> {
        let endpoint: http::Uri = endpoint.parse()?;
        let authority = endpoint
            .authority()
            .as_ref()
            .map(|auth| auth.as_str())
            .unwrap_or("");
        let scheme = *endpoint
            .scheme()
            .as_ref()
            .ok_or_else(|| "endpoint must have scheme")?;
        let new_uri = http::Uri::builder()
            .authority(authority)
            .scheme(scheme.clone())
            .path_and_query(merge_paths(&endpoint, self.request.uri()).as_ref())
            .build()
            .map_err(|_| "failed to construct url")?;
        *self.request.uri_mut() = new_uri;
        Ok(())
    }

    pub fn add_extension<T: Send + Sync + Clone + 'static>(&mut self, extension: T) {
        self.request.extensions_mut().insert(extension);
    }

    pub fn uri(&self) -> Uri<'_> {
        Uri {
            uri: self.request.uri(),
        }
    }

    pub fn body(&self) -> &B {
        self.request.body()
    }

    pub fn remove_header(&mut self, key: &str) {
        self.request.headers_mut().remove(key);
    }

    pub fn add_header<T: AsRef<[u8]> + 'static>(
        &mut self,
        name: impl Into<Cow<'static, str>>,
        value: T,
    ) {
        let headername = match name.into() {
            Cow::Borrowed(b) => HeaderName::from_static(b),
            Cow::Owned(b) => HeaderName::try_from(b).expect("invalid header"),
        };
        self.request.headers_mut().append(
            headername,
            HeaderValue::from_maybe_shared(value).expect("invalid header value"),
        );
    }

    pub fn headers(&self) -> HeadersView {
        HeadersView {
            headers: self.request.headers(),
        }
    }

    pub fn headers_mut(&mut self) -> MutHeadersView {
        MutHeadersView {
            headers: self.request.headers_mut(),
        }
    }
}

pub struct HeadersView<'a> {
    headers: &'a HeaderMap<HeaderValue>,
}

impl HeadersView<'_> {
    pub fn get(&self, key: &str) -> Option<HeaderValueView> {
        self.headers
            .get(key)
            .map(|v| HeaderValueView { header_value: v })
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.headers.contains_key(key)
    }
}

#[derive(Debug)]
pub struct HeaderValueView<'a> {
    header_value: &'a HeaderValue,
}

pub struct MutHeadersView<'a> {
    headers: &'a mut HeaderMap<HeaderValue>,
}

impl MutHeadersView<'_> {
    pub fn append<T: AsRef<[u8]> + 'static>(
        &mut self,
        name: impl Into<Cow<'static, str>>,
        value: T,
    ) {
        let headername = match name.into() {
            Cow::Borrowed(b) => HeaderName::from_static(b),
            Cow::Owned(b) => HeaderName::try_from(b).expect("invalid header"),
        };
        self.headers
            .append(headername, HeaderValue::from_maybe_shared(value).unwrap());
    }

    pub fn insert<T: AsRef<[u8]> + 'static>(
        &mut self,
        name: impl Into<Cow<'static, str>>,
        value: T,
    ) {
        let headername = match name.into() {
            Cow::Borrowed(b) => HeaderName::from_static(b),
            Cow::Owned(b) => HeaderName::try_from(b).expect("invalid header"),
        };
        self.headers
            .insert(headername, HeaderValue::from_maybe_shared(value).unwrap());
    }
}

impl HeaderValueView<'_> {
    pub fn to_str(&self) -> Result<&str, BoxError> {
        Ok(self.header_value.to_str()?)
    }
}

/*
impl<T> PartialEq<T> for HeaderValueView<'_>
where
    T: PartialEq<HeaderValue>,
{
    fn eq(&self, other: &T) -> bool {
        PartialEq::eq(other, self.header_value)
    }
}*/

impl<'a, 'b, T: ?Sized> PartialEq<&'a T> for HeaderValueView<'b>
where
    HeaderValue: PartialEq<T>,
{
    #[inline]
    fn eq(&self, other: &&'a T) -> bool {
        *self.header_value == **other
    }
}

impl<'a> PartialEq<HeaderValueView<'_>> for &'a str {
    #[inline]
    fn eq(&self, other: &HeaderValueView<'_>) -> bool {
        *other == *self
    }
}

/*
impl<'a> PartialEq<&'a str> for HeaderValueView<'_> {
    fn eq(&self, other: &str) -> bool {
        self.header_value.as_bytes() == other.as_bytes()
    }
}*/

impl Request<SdkBody> {
    pub fn try_clone(&self) -> Option<Self> {
        let request = &self.request;
        let cloned_body = request.body().try_clone()?;
        let mut cloned_request = ::http::Request::builder()
            .uri(request.uri().clone())
            .method(request.method());
        *cloned_request
            .headers_mut()
            .expect("builder has not been modified, headers must be valid") =
            request.headers().clone();
        Some(Self {
            request: cloned_request
                .body(cloned_body)
                .expect("a clone of a valid request should be a valid request"),
        })
    }
}

#[derive(Debug, Clone)]
pub struct Uri<'a> {
    uri: &'a http::Uri,
}

fn merge_paths<'a>(endpoint: &'a http::Uri, uri: &'a http::Uri) -> Cow<'a, str> {
    if let Some(query) = endpoint.path_and_query().and_then(|pq| pq.query()) {
        tracing::warn!(query = %query, "query specified in endpoint will be ignored during endpoint resolution");
    }
    let endpoint_path = endpoint.path();
    let uri_path_and_query = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("");
    if endpoint_path.is_empty() {
        Cow::Borrowed(uri_path_and_query)
    } else {
        let ep_no_slash = endpoint_path.strip_suffix('/').unwrap_or(endpoint_path);
        let uri_path_no_slash = uri_path_and_query
            .strip_prefix('/')
            .unwrap_or(uri_path_and_query);
        Cow::Owned(format!("{}/{}", ep_no_slash, uri_path_no_slash))
    }
}

#[cfg(test)]
mod test {
    use crate::client::http::HeaderValueView;
    use http::HeaderValue;

    #[test]
    fn str_equality() {
        let value = HeaderValue::from_static("value");
        let header = HeaderValueView {
            header_value: &value,
        };
        assert_eq!(header, "value");
        assert_eq!("value", header);
    }
}
