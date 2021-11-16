use crate::{body::BoxBody, handler::Handler};
use futures_util::{
    future::{BoxFuture, Map},
    FutureExt,
};
use http::{Request, Response};
use std::{
    convert::Infallible,
    marker::PhantomData,
    task::{Context, Poll},
};
use tower::Service;

/// Struct that holds a handler, that is, a function provided by the user that implements the
/// Smithy operation.
pub struct OperationHandler<H, B, T, I, Res> {
    handler: H,
    #[allow(clippy::type_complexity)]
    _marker: PhantomData<fn() -> (B, T, I, Res)>,
}

impl<H, B, T, I, Res> Clone for OperationHandler<H, B, T, I, Res>
where
    H: Clone,
{
    fn clone(&self) -> Self {
        Self { handler: self.handler.clone(), _marker: PhantomData }
    }
}

/// Construct an [`OperationHandler`] out of a function implementing the operation.
pub fn operation<H, B, T, I, Res>(handler: H) -> OperationHandler<H, B, T, I, Res> {
    OperationHandler { handler, _marker: PhantomData }
}

impl<H, B, T, I, Res> Service<Request<B>> for OperationHandler<H, B, T, I, Res>
where
    H: Handler<B, T, I, Res>,
    B: Send + 'static,
{
    type Response = Response<BoxBody>;
    type Error = Infallible;
    type Future = OperationHandlerFuture;

    #[inline]
    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let future = Handler::call(self.handler.clone(), req).map(Ok::<_, Infallible> as _);
        OperationHandlerFuture::new(future)
    }
}

type WrapResultInResponseFn = fn(Response<BoxBody>) -> Result<Response<BoxBody>, Infallible>;

opaque_future! {
    /// Response future for [`OperationHandler`].
    pub type OperationHandlerFuture =
        Map<BoxFuture<'static, Response<BoxBody>>, WrapResultInResponseFn>;
}
