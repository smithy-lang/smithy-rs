/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
//! Portions of the implementation are adapted from axum
//! (<https://github.com/tokio-rs/axum>), which is licensed under the MIT License.
//! Copyright (c) 2019 Axum Contributors

use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use pin_project_lite::pin_project;
use tokio::{
    io::{self, AsyncRead, AsyncWrite},
    net::{TcpListener, TcpStream},
    sync::{OwnedSemaphorePermit, Semaphore},
};

/// Types that can listen for connections.
pub trait Listener: Send + 'static {
    /// The listener's IO type.
    type Io: AsyncRead + AsyncWrite + Unpin + Send + 'static;

    /// The listener's address type.
    type Addr: Send;

    /// Accept a new incoming connection to this listener.
    ///
    /// If the underlying accept call can return an error, this function must
    /// take care of logging and retrying.
    fn accept(&mut self) -> impl Future<Output = (Self::Io, Self::Addr)> + Send;

    /// Returns the local address that this listener is bound to.
    fn local_addr(&self) -> io::Result<Self::Addr>;
}

impl Listener for TcpListener {
    type Io = TcpStream;
    type Addr = std::net::SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            match Self::accept(self).await {
                Ok(tup) => return tup,
                Err(e) => handle_accept_error(e).await,
            }
        }
    }

    #[inline]
    fn local_addr(&self) -> io::Result<Self::Addr> {
        Self::local_addr(self)
    }
}

#[cfg(unix)]
impl Listener for tokio::net::UnixListener {
    type Io = tokio::net::UnixStream;
    type Addr = tokio::net::unix::SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            match Self::accept(self).await {
                Ok(tup) => return tup,
                Err(e) => handle_accept_error(e).await,
            }
        }
    }

    #[inline]
    fn local_addr(&self) -> io::Result<Self::Addr> {
        Self::local_addr(self)
    }
}

/// Extensions to [`Listener`].
pub trait ListenerExt: Listener + Sized {
    /// Limit the number of concurrent connections. Once the limit has
    /// been reached, no additional connections will be accepted until
    /// an existing connection is closed. Listener implementations will
    /// typically continue to queue incoming connections, up to an OS
    /// and implementation-specific listener backlog limit.
    ///
    /// Compare [`tower::limit::concurrency`], which provides ways to
    /// limit concurrent in-flight requests, but does not limit connections
    /// that are idle or in the process of sending request headers.
    ///
    /// [`tower::limit::concurrency`]: https://docs.rs/tower/latest/tower/limit/concurrency/
    fn limit_connections(self, limit: usize) -> ConnLimiter<Self> {
        ConnLimiter {
            listener: self,
            sem: Arc::new(Semaphore::new(limit)),
        }
    }

    /// Run a mutable closure on every accepted `Io`.
    ///
    /// # Example
    ///
    /// ```
    /// use tokio::net::TcpListener;
    /// use aws_smithy_http_server::serve::ListenerExt;
    /// use tracing::trace;
    ///
    /// # async {
    /// let listener = TcpListener::bind("0.0.0.0:3000")
    ///     .await
    ///     .unwrap()
    ///     .tap_io(|tcp_stream| {
    ///         if let Err(err) = tcp_stream.set_nodelay(true) {
    ///             trace!("failed to set TCP_NODELAY on incoming connection: {err:#}");
    ///         }
    ///     });
    /// # };
    /// ```
    fn tap_io<F>(self, tap_fn: F) -> TapIo<Self, F>
    where
        F: FnMut(&mut Self::Io) + Send + 'static,
    {
        TapIo { listener: self, tap_fn }
    }
}

impl<L: Listener> ListenerExt for L {}

#[derive(Debug)]
pub(super) struct ConnectionLimit {
    sem: Arc<Semaphore>,
    max: usize,
}

impl ConnectionLimit {
    pub(super) fn new(max: usize) -> Self {
        Self {
            sem: Arc::new(Semaphore::new(max)),
            max,
        }
    }

    pub(super) fn max(&self) -> usize {
        self.max
    }

    pub(super) fn available_permits(&self) -> usize {
        self.sem.available_permits()
    }

    pub(super) async fn acquire(&self) -> OwnedSemaphorePermit {
        self.sem
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore should never be closed")
    }
}

/// Return type of [`ListenerExt::limit_connections`].
///
/// See that method for details.
#[derive(Debug)]
pub struct ConnLimiter<T> {
    listener: T,
    sem: Arc<Semaphore>,
}

impl<T: Listener> Listener for ConnLimiter<T> {
    type Io = ConnLimiterIo<T::Io>;
    type Addr = T::Addr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        let permit = self
            .sem
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore should never be closed");
        let (io, addr) = self.listener.accept().await;
        (ConnLimiterIo::new(io, Some(permit), None), addr)
    }

    fn local_addr(&self) -> tokio::io::Result<Self::Addr> {
        self.listener.local_addr()
    }
}

pin_project! {
    /// A connection counted by [`ConnLimiter`].
    ///
    /// Also used by [`super::serve`] to retain connection permits across HTTP
    /// upgrades and bound the initial HTTP version detection read.
    ///
    /// The permit is released when this IO object is dropped.
    #[derive(Debug)]
    pub struct ConnLimiterIo<T> {
        #[pin]
        io: T,
        permit: Option<OwnedSemaphorePermit>,
        http_version_detection_deadline: Option<Pin<Box<tokio::time::Sleep>>>,
        preface_bytes_read: usize,
    }
}

impl<T> ConnLimiterIo<T> {
    pub(super) fn new(
        io: T,
        permit: Option<OwnedSemaphorePermit>,
        http_version_detection_timeout: Option<Duration>,
    ) -> Self {
        Self {
            io,
            permit,
            http_version_detection_deadline: http_version_detection_timeout
                .map(|timeout| Box::pin(tokio::time::sleep(timeout))),
            preface_bytes_read: 0,
        }
    }
}

impl<T: AsyncRead> AsyncRead for ConnLimiterIo<T> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut tokio::io::ReadBuf<'_>) -> Poll<io::Result<()>> {
        // Hyper's auto builder waits for this preface (or a mismatch) before
        // constructing a protocol-specific connection and starting its timers.
        const HTTP2_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
        let this = self.project();
        if let Some(deadline) = this.http_version_detection_deadline.as_mut() {
            if deadline.as_mut().poll(cx).is_ready() {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "HTTP version detection timed out",
                )));
            }
        }

        let filled_before = buf.filled().len();
        let result = this.io.poll_read(cx, buf);
        if matches!(&result, Poll::Ready(Ok(()))) && this.http_version_detection_deadline.is_some() {
            let bytes = &buf.filled()[filled_before..];
            let remaining_preface = &HTTP2_PREFACE[*this.preface_bytes_read..];
            let compared = bytes.len().min(remaining_preface.len());
            if bytes[..compared] != remaining_preface[..compared] || compared == remaining_preface.len() {
                // Once Hyper can choose an HTTP version, its own timers take over.
                *this.http_version_detection_deadline = None;
            } else {
                *this.preface_bytes_read += compared;
            }
        }
        result
    }
}

// Simply forward implementation to `io` field.
impl<T: AsyncWrite> AsyncWrite for ConnLimiterIo<T> {
    fn is_write_vectored(&self) -> bool {
        self.io.is_write_vectored()
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().io.poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().io.poll_shutdown(cx)
    }

    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        self.project().io.poll_write(cx, buf)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        self.project().io.poll_write_vectored(cx, bufs)
    }
}

/// Return type of [`ListenerExt::tap_io`].
///
/// See that method for details.
pub struct TapIo<L, F> {
    listener: L,
    tap_fn: F,
}

impl<L, F> fmt::Debug for TapIo<L, F>
where
    L: Listener + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TapIo")
            .field("listener", &self.listener)
            .finish_non_exhaustive()
    }
}

impl<L, F> Listener for TapIo<L, F>
where
    L: Listener,
    F: FnMut(&mut L::Io) + Send + 'static,
{
    type Io = L::Io;
    type Addr = L::Addr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        let (mut io, addr) = self.listener.accept().await;
        (self.tap_fn)(&mut io);
        (io, addr)
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.listener.local_addr()
    }
}

async fn handle_accept_error(e: io::Error) {
    if is_connection_error(&e) {
        return;
    }

    // [From `hyper::Server` in 0.14](https://github.com/hyperium/hyper/blob/v0.14.27/src/server/tcp.rs#L186)
    //
    // > A possible scenario is that the process has hit the max open files
    // > allowed, and so trying to accept a new connection will fail with
    // > `EMFILE`. In some cases, it's preferable to just wait for some time, if
    // > the application will likely close some files (or connections), and try
    // > to accept the connection again. If this option is `true`, the error
    // > will be logged at the `error` level, since it is still a big deal,
    // > and then the listener will sleep for 1 second.
    //
    // hyper allowed customizing this but axum does not.
    tracing::error!("accept error: {e}");
    tokio::time::sleep(Duration::from_secs(1)).await;
}

fn is_connection_error(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::ConnectionRefused | io::ErrorKind::ConnectionAborted | io::ErrorKind::ConnectionReset
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test(start_paused = true)]
    async fn http_version_detection_deadline_does_not_reset_when_bytes_arrive() {
        let (io, mut client) = tokio::io::duplex(64);
        let mut io = ConnLimiterIo::new(io, None, Some(super::super::DEFAULT_HTTP_VERSION_DETECTION_TIMEOUT));
        let mut byte = [0];

        client.write_all(b"P").await.unwrap();
        io.read_exact(&mut byte).await.unwrap();
        tokio::time::advance(Duration::from_secs(29)).await;
        client.write_all(b"R").await.unwrap();
        io.read_exact(&mut byte).await.unwrap();
        tokio::time::advance(Duration::from_secs(1)).await;

        let error = io.read_exact(&mut byte).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    #[tokio::test(start_paused = true)]
    async fn http_version_detection_deadline_ends_after_http1_or_complete_http2_preface() {
        let http2_preface = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
        // Exercise every split of the preface, plus bytes beyond the preface.
        for (bytes, split) in [(b"GET /test HTTP/1.1\r\n".as_slice(), 1)]
            .into_iter()
            .chain((1..http2_preface.len()).map(|split| (http2_preface.as_slice(), split)))
            .chain([(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\nextra".as_slice(), 0)])
        {
            let (io, mut client) = tokio::io::duplex(64);
            let mut io = ConnLimiterIo::new(io, None, Some(Duration::from_secs(30)));
            let mut buffer = vec![0; bytes.len()];
            client.write_all(&bytes[..split]).await.unwrap();
            io.read_exact(&mut buffer[..split]).await.unwrap();
            client.write_all(&bytes[split..]).await.unwrap();
            io.read_exact(&mut buffer[split..]).await.unwrap();
            assert_eq!(buffer, bytes);

            tokio::time::advance(Duration::from_secs(31)).await;
            client.write_all(b"later").await.unwrap();
            let mut buffer = [0; 5];
            io.read_exact(&mut buffer).await.unwrap();
            assert_eq!(&buffer, b"later");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn http_version_detection_deadline_can_be_disabled() {
        let (io, mut client) = tokio::io::duplex(64);
        let mut io = ConnLimiterIo::new(io, None, None);
        tokio::time::advance(Duration::from_secs(31)).await;
        client.write_all(b"P").await.unwrap();
        let mut buffer = [0];
        io.read_exact(&mut buffer).await.unwrap();
        assert_eq!(&buffer, b"P");
    }
}
