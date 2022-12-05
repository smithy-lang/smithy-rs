/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use async_trait::async_trait;
use aws_smithy_http_server::{body::BoxBody, opaque_future};
use futures_util::{
    future::{BoxFuture, Map},
    FutureExt,
};
use http::{Request, Response};
use std::marker::PhantomData;
use tower::Service;

/// Struct that holds a handler, that is, a function provided by the user that implements the
/// Smithy operation.
#[deprecated(
    since = "0.52.0",
    note = "`OperationHandler` is part of the older service builder API. This type no longer appears in the public API."
)]
pub struct OperationHandler<H, B, R, I> {
    handler: H,
    #[allow(clippy::type_complexity)]
    _marker: PhantomData<(B, R, I)>,
}

#[allow(deprecated)]
impl<H, B, R, I> Clone for OperationHandler<H, B, R, I>
where
    H: Clone,
{
    fn clone(&self) -> Self {
        Self {
            handler: self.handler.clone(),
            _marker: PhantomData,
        }
    }
}

/// Construct an [`OperationHandler`] out of a function implementing the operation.
#[allow(deprecated)]
#[deprecated(
    since = "0.52.0",
    note = "`OperationHandler` is part of the older service builder API. This type no longer appears in the public API."
)]
pub fn operation<H, B, R, I>(handler: H) -> OperationHandler<H, B, R, I> {
    OperationHandler {
        handler,
        _marker: PhantomData,
    }
}

#[allow(deprecated)]
impl<H, B, R, I> Service<Request<B>> for OperationHandler<H, B, R, I>
where
    H: Handler<B, R, I>,
    B: Send + 'static,
{
    type Response = Response<BoxBody>;
    type Error = std::convert::Infallible;
    type Future = OperationHandlerFuture;

    #[inline]
    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let future =
            Handler::call(self.handler.clone(), req).map(Ok::<_, std::convert::Infallible> as _);
        OperationHandlerFuture::new(future)
    }
}

type WrapResultInResponseFn =
    fn(Response<BoxBody>) -> Result<Response<BoxBody>, std::convert::Infallible>;

opaque_future! {
    /// Response future for [`OperationHandler`].
    pub type OperationHandlerFuture =
        Map<BoxFuture<'static, Response<BoxBody>>, WrapResultInResponseFn>;
}

pub(crate) mod sealed {
    #![allow(unreachable_pub, missing_docs, missing_debug_implementations)]
    pub trait HiddenTrait {}
    pub struct Hidden;
    impl HiddenTrait for Hidden {}
}

#[deprecated(
    since = "0.52.0",
    note = "The inlineable `Handler` is part of the deprecated service builder API. This type no longer appears in the public API."
)]
#[async_trait]
pub trait Handler<B, T, Fut>: Clone + Send + Sized + 'static {
    #[doc(hidden)]
    type Sealed: sealed::HiddenTrait;

    async fn call(self, req: Request<B>) -> Response<BoxBody>;
}
