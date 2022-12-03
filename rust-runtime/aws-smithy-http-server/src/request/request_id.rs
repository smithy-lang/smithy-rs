/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Request IDs
//!
//! Request IDs are of two kinds:
//!
//! * Client Request ID: the request ID sent from a client in the request header
//! * Server Request ID: the unique ID generated server-side for this request
//!
//! Both can be used at the same time and can be used as inputs in your operation handlers.
//! To use [`ClientRequestId`], specify which headers
//! may contain the client request ID.

use std::{
    fmt::Display,
    task::{Context, Poll},
};

use http::request::Parts;
use thiserror::Error;
use tower::{Layer, Service};
use uuid::Uuid;

use crate::{body::BoxBody, response::IntoResponse};

use super::{internal_server_error, FromParts};

/// Opaque type for Server Request IDs.
///
/// If it is missing, the request will be rejected with a `500 Internal Server Error` response.
#[derive(Clone, Debug)]
pub struct ServerRequestId {
    id: Uuid,
}

/// The server request ID has not been added to the [`Request`](http::Request) or has been previously removed.
#[non_exhaustive]
#[derive(Debug, Error)]
#[error("the `ServerRequestId` is not present in the `http::Request`")]
pub struct MissingServerRequestId;

impl ServerRequestId {
    pub fn new() -> Self {
        Self { id: Uuid::new_v4() }
    }
}

impl Display for ServerRequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id)
    }
}

impl<P> FromParts<P> for ServerRequestId {
    type Rejection = MissingServerRequestId;

    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        parts.extensions.remove().ok_or(MissingServerRequestId)
    }
}

impl Default for ServerRequestId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct ServerRequestIdProvider<S> {
    inner: S,
}

pub struct ServerRequestIdProviderLayer {}

impl ServerRequestIdProviderLayer {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for ServerRequestIdProviderLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for ServerRequestIdProviderLayer {
    type Service = ServerRequestIdProvider<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ServerRequestIdProvider { inner }
    }
}

impl<R, S> Service<http::Request<R>> for ServerRequestIdProvider<S>
where
    S: Service<http::Request<R>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: http::Request<R>) -> Self::Future {
        req.extensions_mut().insert(ServerRequestId::new());
        self.inner.call(req)
    }
}

impl<Protocol> IntoResponse<Protocol> for MissingServerRequestId {
    fn into_response(self) -> http::Response<BoxBody> {
        internal_server_error()
    }
}

/// The Client Request ID.
///
/// If it is missing, the request will be rejected with a `500 Internal Server Error` response.
#[derive(Clone, Debug)]
pub struct ClientRequestId {
    id: Box<str>,
}

/// The client request ID has not been added to the [`Request`](http::Request) or has been previously removed.
#[non_exhaustive]
#[derive(Debug, Error)]
#[error("the `ClientRequestId` is not present in the `http::Request`")]
pub struct MissingClientRequestId;

impl ClientRequestId {
    pub fn new(id: Box<str>) -> Self {
        Self { id }
    }
}

impl Display for ClientRequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id)
    }
}

impl<P> FromParts<P> for Option<ClientRequestId> {
    type Rejection = MissingClientRequestId;

    fn from_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        parts.extensions.remove::<Self>().ok_or(MissingClientRequestId)
    }
}

#[derive(Clone)]
pub struct ClientRequestIdProvider<'a, S> {
    inner: S,
    possible_headers: &'a [&'a str],
}

pub struct ClientRequestIdProviderLayer<'a> {
    possible_headers: &'a [&'a str],
}

impl<'a> ClientRequestIdProviderLayer<'a> {
    pub fn new(possible_headers: &'a [&'a str]) -> Self {
        Self { possible_headers }
    }
}

impl<'a, S> Layer<S> for ClientRequestIdProviderLayer<'a> {
    type Service = ClientRequestIdProvider<'a, S>;

    fn layer(&self, inner: S) -> Self::Service {
        ClientRequestIdProvider {
            inner,
            possible_headers: self.possible_headers,
        }
    }
}

impl<'a, R, S> Service<http::Request<R>> for ClientRequestIdProvider<'a, S>
where
    S: Service<http::Request<R>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: http::Request<R>) -> Self::Future {
        let mut id: Option<ClientRequestId> = None;
        for possible_header in self.possible_headers {
            if let Some(value) = req.headers().get(*possible_header) {
                if let Ok(value) = value.to_str() {
                    id = Some(ClientRequestId::new(value.into()));
                    break;
                }
            }
        }
        req.extensions_mut().insert(id);
        self.inner.call(req)
    }
}

impl<Protocol> IntoResponse<Protocol> for MissingClientRequestId {
    fn into_response(self) -> http::Response<BoxBody> {
        internal_server_error()
    }
}
