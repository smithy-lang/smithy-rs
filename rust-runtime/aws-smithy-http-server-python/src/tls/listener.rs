/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::mpsc;
use std::task::{Context, Poll};

use futures::{ready, Stream};
use hyper::server::accept::Accept;
use pin_project_lite::pin_project;
use tls_listener::{AsyncAccept, AsyncTls, TlsListener};

pin_project! {
    /// A wrapper around [TlsListener] that allows changing TLS config via a channel
    /// and ignores incorrect connections (they cause Hyper server to shutdown otherwise).
    pub struct Listener<A: AsyncAccept, T: AsyncTls<A::Connection>> {
        #[pin]
        inner: TlsListener<A, T>,
        new_acceptor_rx: mpsc::Receiver<T>,
    }
}

impl<A: AsyncAccept, T: AsyncTls<A::Connection>> Listener<A, T> {
    pub fn new(tls: T, listener: A, new_acceptor_rx: mpsc::Receiver<T>) -> Self {
        Self {
            inner: TlsListener::new(tls, listener),
            new_acceptor_rx,
        }
    }
}

impl<A, T> Accept for Listener<A, T>
where
    A: AsyncAccept,
    A::Error: std::error::Error,
    T: AsyncTls<A::Connection>,
{
    type Conn = T::Stream;
    type Error = Infallible;

    fn poll_accept(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Conn, Self::Error>>> {
        // Replace current acceptor (it also contains TLS config) if there is a new one
        if let Ok(acceptor) = self.new_acceptor_rx.try_recv() {
            self.as_mut().project().inner.replace_acceptor_pin(acceptor);
        }

        loop {
            match ready!(self.as_mut().project().inner.poll_next(cx)) {
                Some(Ok(conn)) => return Poll::Ready(Some(Ok(conn))),
                Some(Err(err)) => {
                    // Don't propogate errors to Hyper because it causes server to shutdown
                    tracing::error!(error = ?err, "tls connection error");
                }
                None => return Poll::Ready(None),
            }
        }
    }
}
