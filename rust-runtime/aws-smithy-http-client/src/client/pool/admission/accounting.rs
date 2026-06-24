/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Establishing-guard birth: records the commitment to connect before the
//! connector runs, so the `establishing` counter reflects in-flight attempts.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use aws_smithy_runtime_api::box_error::BoxError;
use pin_project_lite::pin_project;
use tower::Service;

use super::super::connection::ConnectCtx;
use super::super::stats::{ConnectionCounters, EstablishingGuard};

/// Creates an [`EstablishingGuard`] before invoking the inner connector,
/// returning the guard alongside the IO handle.
///
/// The guard increments `establishing` at construction and decrements it on
/// drop (if not promoted). Creating it before the connect call ensures the
/// counter reflects the commitment point.
pub(crate) struct ConnectAccounting<S> {
    inner: S,
    counters: Arc<ConnectionCounters>,
}

impl<S> ConnectAccounting<S> {
    pub(crate) fn new(inner: S, counters: Arc<ConnectionCounters>) -> Self {
        Self { inner, counters }
    }
}

impl<S: Clone> Clone for ConnectAccounting<S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            counters: self.counters.clone(),
        }
    }
}

impl<S, IO> Service<ConnectCtx> for ConnectAccounting<S>
where
    S: Service<ConnectCtx, Response = IO>,
    S::Error: Into<BoxError>,
{
    type Response = (IO, EstablishingGuard);
    type Error = BoxError;
    type Future = AccountingFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, ctx: ConnectCtx) -> Self::Future {
        // Guard is born here, before the connect runs, so `establishing`
        // reflects the commitment point.
        let establishing = EstablishingGuard::new(self.counters.clone());
        AccountingFuture {
            inner: self.inner.call(ctx),
            establishing: Some(establishing),
        }
    }
}

pin_project! {
    /// Future for [`ConnectAccounting`].
    ///
    /// Holds the [`EstablishingGuard`] for the duration of the connect. On
    /// success the guard is handed to the caller alongside the IO handle; if
    /// the connect fails or the future is dropped, the guard drops with it,
    /// decrementing the `establishing` counter.
    pub(crate) struct AccountingFuture<F> {
        #[pin]
        inner: F,
        establishing: Option<EstablishingGuard>,
    }
}

impl<F, IO, E> Future for AccountingFuture<F>
where
    F: Future<Output = Result<IO, E>>,
    E: Into<BoxError>,
{
    type Output = Result<(IO, EstablishingGuard), BoxError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        match this.inner.poll(cx) {
            Poll::Ready(Ok(io)) => {
                let establishing = this
                    .establishing
                    .take()
                    .expect("AccountingFuture polled after completion");
                Poll::Ready(Ok((io, establishing)))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e.into())),
            Poll::Pending => Poll::Pending,
        }
    }
}
