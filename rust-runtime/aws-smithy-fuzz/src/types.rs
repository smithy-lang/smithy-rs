/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use arbitrary::Arbitrary;
use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::{collections::HashMap, fmt::Debug};

use bytes::Bytes;
use http::{HeaderMap, HeaderName, Method};
use serde::{Deserialize, Serialize};

pub struct Body {
    body: Option<Vec<u8>>,
    trailers: Option<HeaderMap>,
}

impl Body {
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self {
            body: Some(bytes),
            trailers: None,
        }
    }
    pub fn from_static(bytes: &'static [u8]) -> Self {
        Self {
            body: Some(bytes.into()),
            trailers: None,
        }
    }
}

impl http_body::Body for Body {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_data(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        match self.as_mut().body.take() {
            Some(data) => Poll::Ready(Some(Ok(data.into()))),
            None => Poll::Ready(None),
        }
    }

    fn poll_trailers(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
        match self.as_mut().trailers.take() {
            Some(trailers) => Poll::Ready(Ok(Some(trailers))),
            None => Poll::Ready(Ok(None)),
        }
    }
}

#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Eq, Arbitrary)]
pub struct HttpRequest {
    pub uri: String,
    pub method: String,
    pub headers: HashMap<String, Vec<String>>,
    pub trailers: HashMap<String, Vec<String>>,
    pub body: Vec<u8>,
}

#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Debug for HttpResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpResponse")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .field("body", &TryString(&self.body))
            .finish()
    }
}

impl Debug for HttpRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpRequest")
            .field("uri", &self.uri)
            .field("method", &self.method)
            .field("headers", &self.headers)
            .field("body", &TryString(&self.body))
            .finish()
    }
}

struct TryString<'a>(&'a [u8]);
impl Debug for TryString<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let try_cbor = cbor_diag::parse_bytes(self.0);
        let str_rep = match try_cbor {
            Ok(repr) => repr.to_diag_pretty(),
            Err(_e) => String::from_utf8_lossy(self.0).to_string(),
        };
        write!(f, "\"{}\"", str_rep)
    }
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct FuzzResult {
    pub response: HttpResponse,
    pub input: Option<String>,
}

impl FuzzResult {
    pub fn into_bytes(self) -> Vec<u8> {
        bincode::serialize(&self).unwrap()
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        bincode::deserialize(bytes).unwrap()
    }
}

impl HttpRequest {
    pub fn into_http_request_04x(&self) -> Option<http::Request<Body>> {
        let mut builder = http::Request::builder()
            .uri(&self.uri)
            .method(Method::from_bytes(self.method.as_bytes()).ok()?);
        for (key, values) in &self.headers {
            for value in values {
                builder = builder.header(key, value);
            }
        }
        let mut trailers = HeaderMap::new();
        for (k, v) in &self.trailers {
            let header_name: HeaderName = k.parse().ok()?;
            for v in v {
                trailers.append(header_name.clone(), v.parse().ok()?);
            }
        }
        builder
            .body(Body {
                body: Some(self.body.clone()),
                trailers: Some(trailers),
            })
            .ok()
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        bincode::deserialize(bytes).unwrap()
    }

    pub fn from_unknown_bytes(bytes: &[u8]) -> Option<Self> {
        bincode::deserialize(bytes).ok()
    }
}
