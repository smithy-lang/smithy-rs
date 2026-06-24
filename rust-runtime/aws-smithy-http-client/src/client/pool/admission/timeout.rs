/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Bounds the TCP + TLS connector call with the per-request connect timeout.

use std::task::{Context, Poll};

use aws_smithy_runtime_api::box_error::BoxError;
use tower::Service;

use crate::client::timeout::{maybe_timeout_future, MaybeTimeoutFuture, TimeoutKind};

use super::super::connection::ConnectCtx;

/// Applies [`ConnectCtx::connect_timeout`] to the inner connector call.
///
/// When the context carries a timeout, the connector call is bounded by it:
/// exceeding the budget yields an `HTTP connect timeout` error. With no
/// timeout the connector runs unbounded. The timeout covers only the
/// connector call — TCP + TLS negotiation — and nothing above this layer, so
/// time spent acquiring a connection-cap permit or waiting to be paced is not
/// charged against the connect budget.
pub(crate) struct ConnectTimeout<C> {
    inner: C,
}

impl<C> ConnectTimeout<C> {
    pub(crate) fn new(inner: C) -> Self {
        Self { inner }
    }
}

impl<C: Clone> Clone for ConnectTimeout<C> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<C, IO> Service<ConnectCtx> for ConnectTimeout<C>
where
    C: Service<http_1x::Uri, Response = IO>,
    C::Error: Into<BoxError>,
{
    type Response = IO;
    type Error = BoxError;
    type Future = MaybeTimeoutFuture<C::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, ctx: ConnectCtx) -> Self::Future {
        let ConnectCtx {
            uri,
            connect_timeout,
            ..
        } = ctx;
        maybe_timeout_future(
            self.inner.call(uri),
            connect_timeout.as_ref().map(|t| t.duration),
            connect_timeout.as_ref().map(|t| &t.sleep_impl),
            TimeoutKind::Connect,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::timeout::test::NeverConnects;
    use aws_smithy_async::rt::sleep::{SharedAsyncSleep, TokioSleep};
    use std::time::Duration;
    use tower::Service as _;

    use super::super::super::connection::{ConnectCtx, TimeoutContext};

    /// A `connect_timeout` fires when the connector does not resolve within
    /// the budget. `NeverConnects` never resolves; a 500ms timeout must
    /// produce an `HTTP connect timeout occurred after ...` error.
    #[tokio::test(start_paused = true)]
    async fn connect_timeout_fires_on_slow_connector() {
        let mut svc = ConnectTimeout::new(NeverConnects::default());
        let sleep = SharedAsyncSleep::new(TokioSleep::new());
        let ctx = ConnectCtx::new(
            "http://example.com".parse().unwrap(),
            Some(TimeoutContext::new(Duration::from_millis(500), sleep)),
        );
        let err = match svc.call(ctx).await {
            Ok(_) => panic!("connect timeout should fire against a never-resolving connector"),
            Err(err) => err,
        };
        let msg = format!("{err}");
        assert!(
            msg.contains("HTTP connect"),
            "expected `HTTP connect` in error, got: {msg}"
        );
        assert!(
            msg.contains("500ms"),
            "expected `500ms` in error, got: {msg}"
        );
    }

    /// With no `connect_timeout`, the connector is unbounded: a connector that
    /// never resolves leaves the call pending indefinitely (under `start_paused`
    /// the future is still pending after a yield).
    #[tokio::test(start_paused = true)]
    async fn no_timeout_does_not_bound_connector() {
        let mut svc = ConnectTimeout::new(NeverConnects::default());
        let ctx = ConnectCtx::new("http://example.com".parse().unwrap(), None);
        let fut = svc.call(ctx);
        tokio::pin!(fut);
        tokio::select! {
            _ = &mut fut => panic!("future should not resolve without a timeout"),
            _ = tokio::task::yield_now() => {}
        }
    }
}
