/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Limit accepted sockets, including sockets undergoing a TLS handshake.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::{future::BoxFuture, ready};
use hyper::server::accept::Accept;
use pin_project_lite::pin_project;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::{AcquireError, OwnedSemaphorePermit, Semaphore};

pub(crate) const DEFAULT_MAX_CONNECTIONS: usize = 16_384;

pin_project! {
    pub(crate) struct LimitedIncoming<A> {
        #[pin]
        inner: A,
        semaphore: Arc<Semaphore>,
        acquiring: Option<BoxFuture<'static, Result<OwnedSemaphorePermit, AcquireError>>>,
        permit: Option<OwnedSemaphorePermit>,
    }
}

impl<A> LimitedIncoming<A> {
    pub(crate) fn new(inner: A, max_connections: usize) -> Self {
        assert!(max_connections > 0, "max_connections must be positive");
        Self {
            inner,
            semaphore: Arc::new(Semaphore::new(max_connections)),
            acquiring: None,
            permit: None,
        }
    }
}

impl<A: Accept> Accept for LimitedIncoming<A> {
    type Conn = LimitedConnection<A::Conn>;
    type Error = A::Error;

    fn poll_accept(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Conn, Self::Error>>> {
        let this = self.project();
        if this.permit.is_none() {
            let acquiring = this
                .acquiring
                .get_or_insert_with(|| Box::pin(this.semaphore.clone().acquire_owned()));
            *this.permit = Some(
                ready!(acquiring.as_mut().poll(cx))
                    .expect("the connection semaphore is never closed"),
            );
            *this.acquiring = None;
        }

        match ready!(this.inner.poll_accept(cx)) {
            Some(Ok(inner)) => Poll::Ready(Some(Ok(LimitedConnection {
                inner,
                _permit: this
                    .permit
                    .take()
                    .expect("permit acquired before accepting"),
            }))),
            Some(Err(err)) => {
                *this.permit = None;
                Poll::Ready(Some(Err(err)))
            }
            None => {
                *this.permit = None;
                Poll::Ready(None)
            }
        }
    }
}

pin_project! {
    pub(crate) struct LimitedConnection<C> {
        #[pin]
        inner: C,
        // Keep the permit through TLS negotiation and the entire HTTP connection.
        _permit: OwnedSemaphorePermit,
    }
}

impl<C: AsyncRead> AsyncRead for LimitedConnection<C> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.project().inner.poll_read(cx, buf)
    }
}

impl<C: AsyncWrite> AsyncWrite for LimitedConnection<C> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.project().inner.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().inner.poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().inner.poll_shutdown(cx)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        self.project().inner.poll_write_vectored(cx, bufs)
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::net::SocketAddr;
    use std::sync::mpsc;
    use std::time::Duration;

    use hyper::server::conn::{AddrIncoming, AddrStream};
    use hyper::service::{make_service_fn, service_fn};
    use hyper::{Body, Response, Server};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
    use tokio::time::timeout;
    use tokio_rustls::rustls::{
        Certificate, ClientConfig, PrivateKey, RootCertStore, ServerConfig, ServerName,
    };
    use tokio_rustls::{TlsAcceptor, TlsConnector};

    use super::*;
    use crate::tls::listener::Listener as TlsListener;

    // Observe actual TCP accepts, rather than TCP connects (which can complete
    // while a socket is still waiting in the kernel backlog).
    struct ObservedIncoming {
        inner: AddrIncoming,
        accepted: UnboundedSender<()>,
    }

    impl Accept for ObservedIncoming {
        type Conn = AddrStream;
        type Error = io::Error;

        fn poll_accept(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Self::Conn, Self::Error>>> {
            let result = ready!(Pin::new(&mut self.inner).poll_accept(cx));
            if matches!(result, Some(Ok(_))) {
                self.accepted.send(()).unwrap();
            }
            Poll::Ready(result)
        }
    }

    fn incoming(
        limit: usize,
    ) -> (
        SocketAddr,
        LimitedIncoming<ObservedIncoming>,
        UnboundedReceiver<()>,
    ) {
        let inner = AddrIncoming::bind(&"127.0.0.1:0".parse().unwrap()).unwrap();
        let address = inner.local_addr();
        let (accepted, receiver) = unbounded_channel();
        (
            address,
            LimitedIncoming::new(ObservedIncoming { inner, accepted }, limit),
            receiver,
        )
    }

    async fn health_response<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S) {
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut response = Vec::new();
        timeout(Duration::from_secs(2), stream.read_to_end(&mut response))
            .await
            .expect("queued connection must be served after a slot is released")
            .unwrap();
        assert!(response.starts_with(b"HTTP/1.1 200"));
    }

    #[tokio::test]
    async fn http_connection_limit_blocks_until_socket_closes() {
        let (address, incoming, mut accepted) = incoming(1);
        let service = make_service_fn(|_| async {
            Ok::<_, Infallible>(service_fn(|_| async {
                Ok::<_, Infallible>(Response::new(Body::from("healthy")))
            }))
        });
        let server = tokio::spawn(Server::builder(incoming).serve(service));
        let first = TcpStream::connect(address).await.unwrap();
        accepted.recv().await.unwrap();

        let mut second = TcpStream::connect(address).await.unwrap();
        assert!(timeout(Duration::from_millis(100), accepted.recv())
            .await
            .is_err());
        drop(first);
        timeout(Duration::from_secs(2), accepted.recv())
            .await
            .unwrap()
            .unwrap();
        health_response(&mut second).await;
        server.abort();
    }

    #[tokio::test]
    async fn header_deadline_releases_connection_slot() {
        let (address, incoming, mut accepted) = incoming(1);
        let service = make_service_fn(|_| async {
            Ok::<_, Infallible>(service_fn(|_| async {
                Ok::<_, Infallible>(Response::new(Body::empty()))
            }))
        });
        let server = tokio::spawn(
            Server::builder(incoming)
                .http1_header_read_timeout(Duration::from_millis(100))
                .serve(service),
        );
        let mut first = TcpStream::connect(address).await.unwrap();
        first.write_all(b"GET / HTTP/1.1\r\nHost:").await.unwrap();
        accepted.recv().await.unwrap();
        let mut second = TcpStream::connect(address).await.unwrap();

        // Keep the incomplete-header socket alive. Only its deadline can free
        // the slot, allowing a complete request to be served.
        health_response(&mut second).await;
        let mut response = Vec::new();
        timeout(Duration::from_secs(2), first.read_to_end(&mut response))
            .await
            .unwrap()
            .unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn tls_handshakes_and_http_connections_share_connection_limit() {
        let (address, incoming, mut accepted) = incoming(1);
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let der = cert.serialize_der().unwrap();
        let config = ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(
                vec![Certificate(der.clone())],
                PrivateKey(cert.serialize_private_key_der()),
            )
            .unwrap();
        let (_sender, receiver) = mpsc::channel();
        let listener = TlsListener::new(
            TlsAcceptor::from(Arc::new(config)),
            tls_listener::hyper::wrap(incoming),
            receiver,
        );
        let service = make_service_fn(|_| async {
            Ok::<_, Infallible>(service_fn(|_| async {
                Ok::<_, Infallible>(Response::new(Body::empty()))
            }))
        });
        let server = tokio::spawn(Server::builder(listener).serve(service));

        // A client withholding its TLS handshake must still occupy a slot.
        let stalled_handshake = TcpStream::connect(address).await.unwrap();
        accepted.recv().await.unwrap();
        let socket = TcpStream::connect(address).await.unwrap();
        assert!(timeout(Duration::from_millis(100), accepted.recv())
            .await
            .is_err());
        drop(stalled_handshake);
        timeout(Duration::from_secs(2), accepted.recv())
            .await
            .unwrap()
            .unwrap();

        let mut roots = RootCertStore::empty();
        roots.add(&Certificate(der)).unwrap();
        let config = ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let connector = TlsConnector::from(Arc::new(config));
        let mut first = connector
            .connect(ServerName::try_from("localhost").unwrap(), socket)
            .await
            .unwrap();

        // Finishing TLS does not release the slot: it remains held throughout HTTP.
        let queued = TcpStream::connect(address).await.unwrap();
        assert!(timeout(Duration::from_millis(100), accepted.recv())
            .await
            .is_err());
        health_response(&mut first).await;
        drop(first);
        timeout(Duration::from_secs(2), accepted.recv())
            .await
            .unwrap()
            .unwrap();
        let mut second = connector
            .connect(ServerName::try_from("localhost").unwrap(), queued)
            .await
            .unwrap();
        health_response(&mut second).await;
        server.abort();
    }
}
