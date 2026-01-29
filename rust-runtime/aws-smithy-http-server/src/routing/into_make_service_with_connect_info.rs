/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// This code was copied and then modified from Tokio's Axum.

/* Copyright (c) 2021 Tower Contributors
 *
 * Permission is hereby granted, free of charge, to any
 * person obtaining a copy of this software and associated
 * documentation files (the "Software"), to deal in the
 * Software without restriction, including without
 * limitation the rights to use, copy, modify, merge,
 * publish, distribute, sublicense, and/or sell copies of
 * the Software, and to permit persons to whom the Software
 * is furnished to do so, subject to the following
 * conditions:
 *
 * The above copyright notice and this permission notice
 * shall be included in all copies or substantial portions
 * of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
 * ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
 * TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
 * PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
 * SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
 * CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
 * OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
 * IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
 * DEALINGS IN THE SOFTWARE.
 */

//! The [`IntoMakeServiceWithConnectInfo`] is a service factory which adjoins [`ConnectInfo`] to the requests.

use std::{
    convert::Infallible,
    fmt,
    future::ready,
    marker::PhantomData,
    net::SocketAddr,
    task::{Context, Poll},
};

use tower::{Layer, Service};
use tower_http::add_extension::{AddExtension, AddExtensionLayer};

use crate::request::connect_info::ConnectInfo;

/// A [`MakeService`] used to insert [`ConnectInfo<T>`] into [`Request`](http::Request)s.
///
/// The `T` must be derivable from the underlying IO resource using the [`Connected`] trait.
///
/// [`MakeService`]: tower::make::MakeService
pub struct IntoMakeServiceWithConnectInfo<S, C> {
    inner: S,
    _connect_info: PhantomData<fn() -> C>,
}

impl<S, C> IntoMakeServiceWithConnectInfo<S, C> {
    pub fn new(svc: S) -> Self {
        Self {
            inner: svc,
            _connect_info: PhantomData,
        }
    }
}

impl<S, C> fmt::Debug for IntoMakeServiceWithConnectInfo<S, C>
where
    S: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IntoMakeServiceWithConnectInfo")
            .field("inner", &self.inner)
            .finish()
    }
}

impl<S, C> Clone for IntoMakeServiceWithConnectInfo<S, C>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _connect_info: PhantomData,
        }
    }
}

/// Trait that connected IO resources implement and use to produce information
/// about the connection.
///
/// The goal for this trait is to allow users to implement custom IO types that
/// can still provide the same connection metadata.
pub trait Connected<T>: Clone {
    /// Create type holding information about the connection.
    fn connect_info(target: T) -> Self;
}

impl Connected<SocketAddr> for SocketAddr {
    fn connect_info(target: SocketAddr) -> Self {
        target
    }
}

impl<'a, L> Connected<crate::serve::IncomingStream<'a, L>> for SocketAddr
where
    L: crate::serve::Listener<Addr = SocketAddr>,
{
    fn connect_info(target: crate::serve::IncomingStream<'a, L>) -> Self {
        *target.remote_addr()
    }
}

impl<S, C, T> Service<T> for IntoMakeServiceWithConnectInfo<S, C>
where
    S: Clone,
    C: Connected<T>,
{
    type Response = AddExtension<S, ConnectInfo<C>>;
    type Error = Infallible;
    type Future = ResponseFuture<S, C>;

    #[inline]
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, target: T) -> Self::Future {
        let connect_info = ConnectInfo(C::connect_info(target));
        let svc = AddExtensionLayer::new(connect_info).layer(self.inner.clone());
        ResponseFuture::new(ready(Ok(svc)))
    }
}

opaque_future! {
    /// Response future for [`IntoMakeServiceWithConnectInfo`].
    pub type ResponseFuture<S, C> =
        std::future::Ready<Result<AddExtension<S, ConnectInfo<C>>, Infallible>>;
}
