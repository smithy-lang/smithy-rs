/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Serve utilities for running HTTP servers.
//!
//! This module provides a convenient [`serve`] function similar to `axum::serve`
//! for easily serving Tower services with Hyper.
//!
//! Portions of the implementation are adapted from axum
//! (https://github.com/tokio-rs/axum), which is licensed under the MIT License.
//! Copyright (c) 2019 Axum Contributors

use std::convert::Infallible;
use std::error::Error as StdError;
use std::fmt::Debug;
use std::future::{Future, IntoFuture};
use std::io;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use http_body::Body as HttpBody;
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use hyper_util::service::TowerToHyperService;
use tokio::sync::watch;
use tower::{Service, ServiceExt as _};

mod listener;

pub use self::listener::{ConnLimiter, ConnLimiterIo, Listener, ListenerExt, TapIo};

type ConfigureHyperFn = Arc<
    dyn Fn(
            hyper_util::server::conn::auto::Builder<hyper_util::rt::TokioExecutor>,
        ) -> hyper_util::server::conn::auto::Builder<hyper_util::rt::TokioExecutor>
        + Send
        + Sync
        + 'static,
>;

/// An incoming stream that bundles connection information.
///
/// This struct serves as the request type for the `make_service` Tower service,
/// allowing it to access connection-level metadata when creating per-connection services.
///
/// # Purpose
///
/// In Tower/Hyper's model, `make_service` is called once per connection to create
/// a service that handles all HTTP requests on that connection. `IncomingStream`
/// provides the connection information needed to customize service creation based on:
/// - The remote address (for logging or access control)
/// - The underlying IO type (for protocol detection or configuration)
///
/// # Design
///
/// This type holds a **reference** to the IO rather than ownership because:
/// - The actual IO is still needed by Hyper to serve the connection after `make_service` returns
/// - The `make_service` only needs to inspect connection metadata, not take ownership
///
/// Used with [`serve`] and [`crate::routing::IntoMakeServiceWithConnectInfo`].
#[derive(Debug)]
pub struct IncomingStream<'a, L>
where
    L: Listener,
{
    io: &'a TokioIo<L::Io>,
    remote_addr: L::Addr,
}

impl<L> IncomingStream<'_, L>
where
    L: Listener,
{
    /// Get a reference to the inner IO type.
    pub fn io(&self) -> &L::Io {
        self.io.inner()
    }

    /// Returns the remote address that this stream is bound to.
    pub fn remote_addr(&self) -> &L::Addr {
        &self.remote_addr
    }
}

/// Serve the service with the supplied listener.
///
/// This implementation provides zero-cost abstraction for shutdown coordination.
/// When graceful shutdown is not used, there is no runtime overhead - no watch channels
/// are allocated and no `tokio::select!` is used.
///
/// It supports both HTTP/1 as well as HTTP/2.
///
/// This function accepts services wrapped with [`IntoMakeService`] or
/// [`IntoMakeServiceWithConnectInfo`].
///
/// For generated Smithy services, use `.into_make_service()` or
/// `.into_make_service_with_connect_info::<C>()`. For services wrapped with
/// Tower middleware, use `IntoMakeService::new(service)`.
///
/// # Examples
///
/// Serving a Smithy service with a TCP listener:
///
/// ```rust,ignore
/// use tokio::net::TcpListener;
///
/// let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
/// aws_smithy_http_server::serve(listener, app.into_make_service()).await.unwrap();
/// ```
///
/// Serving with middleware applied:
///
/// ```rust,ignore
/// use tokio::net::TcpListener;
/// use tower::Layer;
/// use tower::timeout::TimeoutLayer;
/// use aws_smithy_http_server::routing::IntoMakeService;
///
/// let app = /* ... build service ... */;
/// let app = TimeoutLayer::new(Duration::from_secs(30)).layer(app);
///
/// let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
/// aws_smithy_http_server::serve(listener, IntoMakeService::new(app)).await.unwrap();
/// ```
///
/// For graceful shutdown:
///
/// ```rust,ignore
/// use tokio::net::TcpListener;
/// use tokio::signal;
///
/// let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
/// aws_smithy_http_server::serve(listener, app.into_make_service())
///     .with_graceful_shutdown(async {
///         signal::ctrl_c().await.expect("failed to listen for Ctrl+C");
///     })
///     .await
///     .unwrap();
/// ```
///
/// With connection info:
///
/// ```rust,ignore
/// use tokio::net::TcpListener;
/// use std::net::SocketAddr;
///
/// let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
/// aws_smithy_http_server::serve(
///     listener,
///     app.into_make_service_with_connect_info::<SocketAddr>()
/// )
/// .await
/// .unwrap();
/// ```
pub fn serve<L, M, S, B>(listener: L, make_service: M) -> Serve<L, M, S, B>
where
    L: Listener,
    M: for<'a> Service<IncomingStream<'a, L>, Error = Infallible, Response = S>,
    S: Service<http::Request<Incoming>, Response = http::Response<B>, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
    B: HttpBody + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn StdError + Send + Sync>>,
{
    Serve::new(listener, make_service)
}

/// A server that can be awaited or configured with graceful shutdown.
///
/// Created by [`serve`].
pub struct Serve<L, M, S, B> {
    listener: L,
    make_service: M,
    configure_hyper: Option<ConfigureHyperFn>,
    _marker: PhantomData<(S, B)>,
}

impl<L, M, S, B> Serve<L, M, S, B>
where
    L: Listener,
{
    fn new(listener: L, make_service: M) -> Self {
        Self {
            listener,
            make_service,
            configure_hyper: None,
            _marker: PhantomData,
        }
    }

    /// Configure the underlying Hyper server connection builder.
    pub fn configure_hyper<F>(mut self, f: F) -> Self
    where
        F: Fn(
                hyper_util::server::conn::auto::Builder<hyper_util::rt::TokioExecutor>,
            ) -> hyper_util::server::conn::auto::Builder<hyper_util::rt::TokioExecutor>
            + Send
            + Sync
            + 'static,
    {
        self.configure_hyper = Some(Arc::new(f));
        self
    }

    /// Enable graceful shutdown for the server.
    pub fn with_graceful_shutdown<F>(self, signal: F) -> WithGracefulShutdown<L, M, S, F, B>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        WithGracefulShutdown::new(
            self.listener,
            self.make_service,
            signal,
            self.configure_hyper,
        )
    }

    /// Returns the local address this server is bound to.
    pub fn local_addr(&self) -> io::Result<L::Addr> {
        self.listener.local_addr()
    }
}

// Implement IntoFuture so we can await Serve directly
impl<L, M, S, B> IntoFuture for Serve<L, M, S, B>
where
    L: Listener,
    L::Addr: Debug,
    M: for<'a> Service<IncomingStream<'a, L>, Error = Infallible, Response = S> + Send + 'static,
    for<'a> <M as Service<IncomingStream<'a, L>>>::Future: Send,
    S: Service<http::Request<Incoming>, Response = http::Response<B>, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
    B: HttpBody + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn StdError + Send + Sync>>,
{
    type Output = io::Result<()>;
    type IntoFuture = Pin<Box<dyn Future<Output = io::Result<()>> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let Self {
                mut listener,
                mut make_service,
                configure_hyper,
                _marker,
            } = self;

            loop {
                let (io, remote_addr) = listener.accept().await;
                handle_connection(&mut make_service, io, remote_addr, configure_hyper.clone()).await;
            }
        })
    }
}

/// A server with graceful shutdown enabled.
///
/// Created by [`Serve::with_graceful_shutdown`].
pub struct WithGracefulShutdown<L, M, S, F, B> {
    listener: L,
    make_service: M,
    signal: F,
    configure_hyper: Option<ConfigureHyperFn>,
    shutdown_timeout: Option<Duration>,
    _marker: PhantomData<(S, B)>,
}

impl<L: Listener, M, S, F, B> WithGracefulShutdown<L, M, S, F, B> {
    fn new(
        listener: L,
        make_service: M,
        signal: F,
        configure_hyper: Option<ConfigureHyperFn>,
    ) -> Self
    where
        F: Future<Output = ()> + Send + 'static,
    {
        Self {
            listener,
            make_service,
            signal,
            configure_hyper,
            shutdown_timeout: None,
            _marker: PhantomData,
        }
    }

    /// Configure the underlying Hyper server connection builder.
    pub fn configure_hyper<G>(mut self, f: G) -> Self
    where
        G: Fn(
                hyper_util::server::conn::auto::Builder<hyper_util::rt::TokioExecutor>,
            ) -> hyper_util::server::conn::auto::Builder<hyper_util::rt::TokioExecutor>
            + Send
            + Sync
            + 'static,
    {
        self.configure_hyper = Some(Arc::new(f));
        self
    }

    /// Set a timeout for graceful shutdown.
    pub fn with_shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.shutdown_timeout = Some(timeout);
        self
    }

    /// Returns the local address this server is bound to.
    pub fn local_addr(&self) -> io::Result<L::Addr> {
        self.listener.local_addr()
    }
}

// Implement IntoFuture so we can await WithGracefulShutdown directly
impl<L, M, S, F, B> IntoFuture for WithGracefulShutdown<L, M, S, F, B>
where
    L: Listener,
    L::Addr: Debug,
    M: for<'a> Service<IncomingStream<'a, L>, Error = Infallible, Response = S> + Send + 'static,
    for<'a> <M as Service<IncomingStream<'a, L>>>::Future: Send,
    S: Service<http::Request<Incoming>, Response = http::Response<B>, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
    F: Future<Output = ()> + Send + 'static,
    B: HttpBody + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn StdError + Send + Sync>>,
{
    type Output = io::Result<()>;
    type IntoFuture = Pin<Box<dyn Future<Output = io::Result<()>> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let Self {
                mut listener,
                mut make_service,
                signal,
                configure_hyper,
                shutdown_timeout,
                _marker,
            } = self;

            // Create watch channels for shutdown coordination
            let (signal_tx, signal_rx) = watch::channel(());
            let (close_tx, close_rx) = watch::channel(());

            // Spawn task to wait for shutdown signal
            tokio::spawn(async move {
                signal.await;
                tracing::debug!("received graceful shutdown signal. Telling tasks to shutdown");
                drop(signal_rx);  // Drop the receiver to signal all senders
            });

            loop {
                let (io, remote_addr) = tokio::select! {
                    conn = listener.accept() => conn,
                    _ = signal_tx.closed() => {
                        tracing::debug!("signal received, not accepting new connections");
                        break;
                    }
                };

                handle_connection_graceful(
                    &mut make_service,
                    io,
                    remote_addr,
                    configure_hyper.clone(),
                    signal_tx.clone(),
                    close_rx.clone(),
                )
                .await;
            }

            drop(listener);
            drop(close_rx); // Drop the original receiver before waiting for connections to complete

            tracing::debug!(
                "waiting for {} task(s) to finish",
                close_tx.receiver_count()
            );

            // Wait for all in-flight connections to finish (with optional timeout)
            match shutdown_timeout {
                Some(timeout) => {
                    match tokio::time::timeout(timeout, close_tx.closed()).await {
                        Ok(_) => {
                            tracing::debug!("all in-flight connections completed during graceful shutdown");
                        }
                        Err(_) => {
                            tracing::warn!(
                                timeout_secs = timeout.as_secs(),
                                "graceful shutdown timeout expired, some connections may not have completed"
                            );
                        }
                    }
                }
                None => {
                    close_tx.closed().await;
                    tracing::debug!("all in-flight connections completed during graceful shutdown");
                }
            }

            Ok(())
        })
    }
}

// Common connection handling logic shared by both graceful and non-graceful shutdown
async fn handle_connection_common<L, M, S, B, F, Fut>(
    make_service: &mut M,
    conn_io: <L as Listener>::Io,
    remote_addr: <L as Listener>::Addr,
    configure_hyper: Option<ConfigureHyperFn>,
    serve_fn: F,
) where
    L: Listener,
    L::Addr: Debug,
    M: for<'a> Service<IncomingStream<'a, L>, Error = Infallible, Response = S> + Send + 'static,
    for<'a> <M as Service<IncomingStream<'a, L>>>::Future: Send,
    S: Service<http::Request<Incoming>, Response = http::Response<B>, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
    B: HttpBody + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn StdError + Send + Sync>>,
    F: FnOnce(Builder<TokioExecutor>, TokioIo<L::Io>, TowerToHyperService<S>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let tokio_io = TokioIo::new(conn_io);

    tracing::trace!("connection {remote_addr:?} accepted");

    make_service
        .ready()
        .await
        .expect("make_service error type is Infallible and cannot fail");

    let tower_service = make_service
        .call(IncomingStream {
            io: &tokio_io,
            remote_addr,
        })
        .await
        .expect("make_service error type is Infallible and cannot fail");

    let hyper_service = TowerToHyperService::new(tower_service);

    tokio::spawn(async move {
        let mut builder = Builder::new(TokioExecutor::new());
        if let Some(configure) = configure_hyper {
            builder = configure(builder);
        }

        // Call the provided closure to handle the actual connection serving
        serve_fn(builder, tokio_io, hyper_service).await;
    });
}

// Version without graceful shutdown - zero cost!
async fn handle_connection<L, M, S, B>(
    make_service: &mut M,
    conn_io: <L as Listener>::Io,
    remote_addr: <L as Listener>::Addr,
    configure_hyper: Option<ConfigureHyperFn>,
) where
    L: Listener,
    L::Addr: Debug,
    M: for<'a> Service<IncomingStream<'a, L>, Error = Infallible, Response = S> + Send + 'static,
    for<'a> <M as Service<IncomingStream<'a, L>>>::Future: Send,
    S: Service<http::Request<Incoming>, Response = http::Response<B>, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
    B: HttpBody + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn StdError + Send + Sync>>,
{
    handle_connection_common::<L, M, S, B, _, _>(
        make_service,
        conn_io,
        remote_addr,
        configure_hyper,
        |builder, tokio_io, hyper_service| async move {
            let conn = builder.serve_connection_with_upgrades(tokio_io, hyper_service);
            if let Err(err) = conn.await {
                tracing::error!(error = ?err, "error serving connection");
            }
        },
    )
    .await;
}

// Version with graceful shutdown
async fn handle_connection_graceful<L, M, S, B>(
    make_service: &mut M,
    conn_io: <L as Listener>::Io,
    remote_addr: <L as Listener>::Addr,
    configure_hyper: Option<ConfigureHyperFn>,
    signal_tx: watch::Sender<()>,
    close_rx: watch::Receiver<()>,
) where
    L: Listener,
    L::Addr: Debug,
    M: for<'a> Service<IncomingStream<'a, L>, Error = Infallible, Response = S> + Send + 'static,
    for<'a> <M as Service<IncomingStream<'a, L>>>::Future: Send,
    S: Service<http::Request<Incoming>, Response = http::Response<B>, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
    B: HttpBody + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn StdError + Send + Sync>>,
{
    handle_connection_common::<L, M, S, B, _, _>(
        make_service,
        conn_io,
        remote_addr,
        configure_hyper,
        move |builder, tokio_io, hyper_service| async move {
            let _close_guard = close_rx; // Will be dropped when this async block completes

            let conn = builder.serve_connection_with_upgrades(tokio_io, hyper_service);
            let mut conn = std::pin::pin!(conn);

            // Use tokio::select! for graceful shutdown
            loop {
                tokio::select! {
                    result = conn.as_mut() => {
                        if let Err(err) = result {
                            tracing::error!(error = ?err, "error serving connection");
                        }
                        break;
                    }
                    _ = signal_tx.closed() => {
                        tracing::trace!("signal received in task, starting graceful shutdown");
                        conn.as_mut().graceful_shutdown();
                    }
                }
            }
        },
    )
    .await;
}
