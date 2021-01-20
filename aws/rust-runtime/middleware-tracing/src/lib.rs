use std::fmt::Debug;
use std::task::{Context, Poll};
use tower::Service;
use tracing::instrument::{Instrument, Instrumented};
use tracing::{span, Level};

#[derive(Clone)]
pub struct RawRequestLogging<S> {
    pub inner: S,
}

impl<S, B> Service<http::Request<B>> for RawRequestLogging<S>
where
    B: Debug,
    S: Service<http::Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Instrumented<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        let span = span!(Level::TRACE, "request_dispatch", req = ?&req);
        self.inner.call(req).instrument(span)
    }
}
