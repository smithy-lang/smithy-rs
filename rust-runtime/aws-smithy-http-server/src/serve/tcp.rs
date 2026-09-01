/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::convert::Infallible;
use std::error::Error as StdError;
use std::fmt::{self, Debug};
use std::future::{Future, IntoFuture};
use std::io;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use http_body::Body as HttpBody;
use hyper::body::Incoming;
use hyper_util::rt::TokioExecutor;
use hyper_util::server::conn::auto::Builder;
use tokio::net::{lookup_host, TcpListener, TcpSocket, ToSocketAddrs};
use tower::Service;

use super::{serve, IncomingStream, Serve};

/// Default kernel socket listen backlog for TCP listeners created by [`bind`].
///
/// The effective backlog may be capped by OS settings such as Linux
/// `net.core.somaxconn`.
pub const DEFAULT_SOCKET_LISTEN_BACKLOG: u32 = 1024;

const DEFAULT_TCP_NODELAY: bool = true;

const DEFAULT_TCP_KEEPALIVE: bool = true;

/// Create a TCP-binding serve builder.
///
/// This binds a TCP listener when awaited, then delegates to [`serve`] so the
/// existing server defaults and configuration behavior are reused.
///
/// `addr` accepts any [`ToSocketAddrs`] value, including `"127.0.0.1:3000"`,
/// `("127.0.0.1", 3000)`, and `std::net::SocketAddr`.
///
/// # Example
///
/// ```rust,ignore
/// aws_smithy_http_server::serve::bind(("127.0.0.1", 3000), app.into_make_service())
///     .socket_listen_backlog(2048)
///     .tcp_nodelay(true)
///     .tcp_keepalive(true)
///     .max_connections(1024)
///     .with_graceful_shutdown(shutdown_signal())
///     .await?;
/// ```
pub fn bind<A, M, S, B>(addr: A, make_service: M) -> Bind<A, M, S, B> {
    Bind::new(addr, make_service)
}

/// A TCP-binding serve builder.
///
/// `Bind` owns TCP socket creation settings. When awaited, it binds a
/// [`TcpListener`], delegates to [`serve`], applies any configured server
/// settings, and awaits the resulting server future.
#[must_use = "Bind does nothing until you `.await`, call `.into_future()`, or call `.into_serve()`"]
pub struct Bind<A, M, S, B> {
    addr: A,
    make_service: M,
    config: BindConfig,
    _marker: PhantomData<(S, B)>,
}

impl<A, M, S, B> fmt::Debug for Bind<A, M, S, B>
where
    A: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bind")
            .field("addr", &self.addr)
            .field("socket_listen_backlog", &self.config.socket_listen_backlog)
            .field("tcp_nodelay", &self.config.tcp_nodelay)
            .field("tcp_keepalive", &self.config.tcp_keepalive)
            .field("has_hyper_config", &self.config.hyper_builder.is_some())
            .field("max_connections", &self.config.max_connections)
            .finish_non_exhaustive()
    }
}

impl<A, M, S, B> Bind<A, M, S, B> {
    fn new(addr: A, make_service: M) -> Self {
        Self {
            addr,
            make_service,
            config: BindConfig::default(),
            _marker: PhantomData,
        }
    }

    /// Set the kernel socket listen backlog passed to [`TcpSocket::listen`].
    ///
    /// This controls how many pending, established TCP connections the OS may
    /// queue before the server accepts them. It is distinct from
    /// [`max_connections`](Self::max_connections), which limits concurrently
    /// accepted connections handled by the server.
    ///
    /// The effective backlog may be capped by OS settings such as Linux
    /// `net.core.somaxconn`.
    pub fn socket_listen_backlog(mut self, backlog: u32) -> Self {
        self.config.socket_listen_backlog = backlog;
        self
    }

    /// Set `TCP_NODELAY` on the TCP listener socket.
    ///
    /// This is enabled by default for listeners created by [`bind`].
    pub fn tcp_nodelay(mut self, enabled: bool) -> Self {
        self.config.tcp_nodelay = enabled;
        self
    }

    /// Set `SO_KEEPALIVE` on the TCP listener socket.
    ///
    /// This is enabled by default for listeners created by [`bind`]. The
    /// platform controls keep-alive probe timing unless the socket is created
    /// manually and passed to [`serve`].
    pub fn tcp_keepalive(mut self, enabled: bool) -> Self {
        self.config.tcp_keepalive = enabled;
        self
    }

    /// Configure the underlying Hyper connection builder.
    ///
    /// Calling this replaces the default builder entirely, matching
    /// [`Serve::configure_hyper`].
    pub fn configure_hyper<F>(mut self, f: F) -> Self
    where
        F: FnOnce(Builder<TokioExecutor>) -> Builder<TokioExecutor>,
    {
        self.config.hyper_builder = Some(Arc::new(f(Builder::new(TokioExecutor::new()))));
        self
    }

    /// Set the maximum number of concurrent accepted connections.
    pub fn max_connections(mut self, max: usize) -> Self {
        self.config.max_connections = Some(Some(max));
        self
    }

    /// Disable the default accepted-connection limit.
    pub fn disable_connection_limit(mut self) -> Self {
        self.config.max_connections = Some(None);
        self
    }

    /// Enable graceful shutdown for the server.
    pub fn with_graceful_shutdown<F>(self, signal: F) -> BindWithGracefulShutdown<A, M, S, F, B>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        BindWithGracefulShutdown {
            tcp_serve: self,
            signal,
            shutdown_timeout: None,
        }
    }
}

impl<A, M, S, B> Bind<A, M, S, B>
where
    A: ToSocketAddrs,
    B: HttpBody + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn StdError + Send + Sync>>,
    S: Service<http::Request<Incoming>, Response = http::Response<B>, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
    M: for<'a> Service<IncomingStream<'a, TcpListener>, Error = Infallible, Response = S>,
{
    /// Bind the TCP listener and return the existing [`Serve`] builder.
    ///
    /// Use this when you need to inspect [`Serve::local_addr`] before running
    /// the server, such as when binding to port `0` in tests.
    pub async fn into_serve(self) -> io::Result<Serve<TcpListener, M, S, B>> {
        let Self {
            addr,
            make_service,
            config,
            _marker,
        } = self;
        let listener = bind_tcp_listener(addr, &config).await?;
        let mut serve = serve(listener, make_service);
        config.apply_to_serve(&mut serve);
        Ok(serve)
    }
}

impl<A, M, S, B> IntoFuture for Bind<A, M, S, B>
where
    A: ToSocketAddrs + Send + 'static,
    B: HttpBody + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn StdError + Send + Sync>>,
    S: Service<http::Request<Incoming>, Response = http::Response<B>, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
    M: for<'a> Service<IncomingStream<'a, TcpListener>, Error = Infallible, Response = S> + Send + 'static,
    for<'a> <M as Service<IncomingStream<'a, TcpListener>>>::Future: Send,
{
    type Output = io::Result<()>;
    type IntoFuture = Pin<Box<dyn Future<Output = io::Result<()>> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move { self.into_serve().await?.await })
    }
}

/// A bind builder with graceful shutdown enabled.
#[must_use = "BindWithGracefulShutdown does nothing until you `.await` or call `.into_future()`"]
pub struct BindWithGracefulShutdown<A, M, S, F, B> {
    tcp_serve: Bind<A, M, S, B>,
    signal: F,
    shutdown_timeout: Option<Duration>,
}

impl<A, M, S, F, B> fmt::Debug for BindWithGracefulShutdown<A, M, S, F, B>
where
    A: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BindWithGracefulShutdown")
            .field("tcp_serve", &self.tcp_serve)
            .field("shutdown_timeout", &self.shutdown_timeout)
            .finish_non_exhaustive()
    }
}

impl<A, M, S, F, B> BindWithGracefulShutdown<A, M, S, F, B> {
    /// Set the kernel socket listen backlog passed to [`TcpSocket::listen`].
    pub fn socket_listen_backlog(mut self, backlog: u32) -> Self {
        self.tcp_serve.config.socket_listen_backlog = backlog;
        self
    }

    /// Set `TCP_NODELAY` on the TCP listener socket.
    pub fn tcp_nodelay(mut self, enabled: bool) -> Self {
        self.tcp_serve.config.tcp_nodelay = enabled;
        self
    }

    /// Set `SO_KEEPALIVE` on the TCP listener socket.
    pub fn tcp_keepalive(mut self, enabled: bool) -> Self {
        self.tcp_serve.config.tcp_keepalive = enabled;
        self
    }

    /// Configure the underlying Hyper connection builder.
    pub fn configure_hyper<G>(mut self, f: G) -> Self
    where
        G: FnOnce(Builder<TokioExecutor>) -> Builder<TokioExecutor>,
    {
        self.tcp_serve.config.hyper_builder = Some(Arc::new(f(Builder::new(TokioExecutor::new()))));
        self
    }

    /// Set the maximum number of concurrent accepted connections.
    pub fn max_connections(mut self, max: usize) -> Self {
        self.tcp_serve.config.max_connections = Some(Some(max));
        self
    }

    /// Disable the default accepted-connection limit.
    pub fn disable_connection_limit(mut self) -> Self {
        self.tcp_serve.config.max_connections = Some(None);
        self
    }

    /// Set a timeout for graceful shutdown.
    pub fn with_shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.shutdown_timeout = Some(timeout);
        self
    }
}

impl<A, M, S, F, B> IntoFuture for BindWithGracefulShutdown<A, M, S, F, B>
where
    A: ToSocketAddrs + Send + 'static,
    B: HttpBody + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn StdError + Send + Sync>>,
    S: Service<http::Request<Incoming>, Response = http::Response<B>, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
    M: for<'a> Service<IncomingStream<'a, TcpListener>, Error = Infallible, Response = S> + Send + 'static,
    for<'a> <M as Service<IncomingStream<'a, TcpListener>>>::Future: Send,
    F: Future<Output = ()> + Send + 'static,
{
    type Output = io::Result<()>;
    type IntoFuture = Pin<Box<dyn Future<Output = io::Result<()>> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let Self {
                tcp_serve,
                signal,
                shutdown_timeout,
            } = self;
            let mut serve = tcp_serve.into_serve().await?.with_graceful_shutdown(signal);
            if let Some(timeout) = shutdown_timeout {
                serve = serve.with_shutdown_timeout(timeout);
            }
            serve.await
        })
    }
}

#[derive(Debug)]
struct BindConfig {
    socket_listen_backlog: u32,
    tcp_nodelay: bool,
    tcp_keepalive: bool,
    hyper_builder: Option<Arc<Builder<TokioExecutor>>>,
    max_connections: Option<Option<usize>>,
}

impl Default for BindConfig {
    fn default() -> Self {
        Self {
            socket_listen_backlog: DEFAULT_SOCKET_LISTEN_BACKLOG,
            tcp_nodelay: DEFAULT_TCP_NODELAY,
            tcp_keepalive: DEFAULT_TCP_KEEPALIVE,
            hyper_builder: None,
            max_connections: None,
        }
    }
}

impl BindConfig {
    fn apply_to_serve<M, S, B>(self, serve: &mut Serve<TcpListener, M, S, B>) {
        if let Some(hyper_builder) = self.hyper_builder {
            serve.hyper_builder = Some(hyper_builder);
        }
        if let Some(max_connections) = self.max_connections {
            serve.max_connections = max_connections;
        }
    }
}

async fn bind_tcp_listener<A>(addr: A, config: &BindConfig) -> io::Result<TcpListener>
where
    A: ToSocketAddrs,
{
    let addrs = lookup_host(addr).await?;
    let mut last_err = None;

    for addr in addrs {
        let socket = match if addr.is_ipv4() {
            TcpSocket::new_v4()
        } else {
            TcpSocket::new_v6()
        } {
            Ok(socket) => socket,
            Err(err) => {
                last_err = Some(err);
                continue;
            }
        };

        if let Err(err) = socket.set_nodelay(config.tcp_nodelay) {
            last_err = Some(err);
            continue;
        }

        if let Err(err) = socket.set_keepalive(config.tcp_keepalive) {
            last_err = Some(err);
            continue;
        }

        if let Err(err) = socket.bind(addr) {
            last_err = Some(err);
            continue;
        }

        match socket.listen(config.socket_listen_backlog) {
            Ok(listener) => return Ok(listener),
            Err(err) => last_err = Some(err),
        }
    }

    Err(last_err.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "address resolution did not return any socket addresses",
        )
    }))
}
