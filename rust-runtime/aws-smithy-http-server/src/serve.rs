/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Serve utilities for running HTTP servers.
//!
//! This module provides a convenient [`serve`] function similar to `axum::serve`
//! for easily serving Tower services with Hyper.
//!
//! Portions of the graceful shutdown implementation are adapted from axum
//! (https://github.com/tokio-rs/axum), which is licensed under the MIT License.
//! Copyright (c) 2019 Axum Contributors

use std::convert::Infallible;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tower::Service;

type ConfigureHyperFn = Arc<
    dyn Fn(
            hyper_util::server::conn::auto::Builder<hyper_util::rt::TokioExecutor>,
        ) -> hyper_util::server::conn::auto::Builder<hyper_util::rt::TokioExecutor>
        + Send
        + Sync
        + 'static,
>;

/// Serve a service on the given listener.
///
/// This is a convenience function that automatically handles:
/// - HTTP/1 and HTTP/2 protocol detection
/// - Connection accept loop
/// - Spawning tasks for each connection
/// - Protocol upgrades (e.g., WebSocket)
///
/// # Example
///
/// ```rust,ignore
/// use tokio::net::TcpListener;
///
/// let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
/// aws_smithy_http_server::serve(listener, app).await.unwrap();
/// ```
///
/// For graceful shutdown support:
///
/// ```rust,ignore
/// use tokio::net::TcpListener;
/// use tokio::signal;
///
/// let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
/// aws_smithy_http_server::serve(listener, app)
///     .with_graceful_shutdown(async {
///         signal::ctrl_c().await.expect("failed to listen for Ctrl+C");
///     })
///     .await
///     .unwrap();
/// ```
pub fn serve<S>(listener: tokio::net::TcpListener, service: S) -> Serve<S>
where
    S: Service<http::Request<hyper::body::Incoming>, Response = http::Response<crate::body::BoxBody>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send,
{
    Serve::new(listener, service)
}

/// A server that can be awaited or configured with graceful shutdown.
///
/// Created by [`serve`].
pub struct Serve<S> {
    listener: tokio::net::TcpListener,
    service: S,
    enable_connect_info: bool,
    configure_hyper: Option<ConfigureHyperFn>,
}

impl<S> Serve<S> {
    fn new(listener: tokio::net::TcpListener, service: S) -> Self {
        Self {
            listener,
            service,
            enable_connect_info: false,
            configure_hyper: None,
        }
    }

    /// Enable `ConnectInfo` extraction in request handlers.
    ///
    /// When enabled, the remote address will be available in handlers via
    /// `ConnectInfo<SocketAddr>` extraction.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use tokio::net::TcpListener;
    /// use aws_smithy_http_server::request::connect_info::ConnectInfo;
    ///
    /// let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    /// aws_smithy_http_server::serve(listener, app)
    ///     .with_connect_info()
    ///     .await
    ///     .unwrap();
    ///
    /// // In your handler:
    /// // fn handler(input: Input, connect_info: ConnectInfo<SocketAddr>) -> Output {
    /// //     println!("Request from: {}", connect_info.0);
    /// //     ...
    /// // }
    /// ```
    pub fn with_connect_info(mut self) -> Self {
        self.enable_connect_info = true;
        self
    }

    /// Configure the underlying Hyper server connection builder.
    ///
    /// This allows customization of Hyper settings like:
    /// - `http1_max_buf_size()`
    /// - `http2_max_concurrent_streams()`
    /// - Other protocol-specific settings
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use tokio::net::TcpListener;
    ///
    /// let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    /// aws_smithy_http_server::serve(listener, app)
    ///     .configure_hyper(|builder| {
    ///         builder
    ///             .http1_max_buf_size(16 * 1024)
    ///             .http2_max_concurrent_streams(200)
    ///     })
    ///     .await
    ///     .unwrap();
    /// ```
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
    ///
    /// When the provided future completes, the server will stop accepting new connections
    /// and wait for existing connections to complete.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use tokio::net::TcpListener;
    /// use tokio::signal;
    ///
    /// let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    /// aws_smithy_http_server::serve(listener, app)
    ///     .with_graceful_shutdown(shutdown_signal())
    ///     .await
    ///     .unwrap();
    ///
    /// async fn shutdown_signal() {
    ///     signal::ctrl_c()
    ///         .await
    ///         .expect("failed to install Ctrl+C signal handler");
    /// }
    /// ```
    pub fn with_graceful_shutdown<F>(self, signal: F) -> WithGracefulShutdown<S, F>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        WithGracefulShutdown::new(
            self.listener,
            self.service,
            signal,
            self.enable_connect_info,
            self.configure_hyper,
        )
    }
}

// Implement IntoFuture so we can await Serve directly
impl<S> std::future::IntoFuture for Serve<S>
where
    S: Service<http::Request<hyper::body::Incoming>, Response = http::Response<crate::body::BoxBody>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send,
{
    type Output = io::Result<()>;
    type IntoFuture = Pin<Box<dyn Future<Output = io::Result<()>> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let configure_hyper = self.configure_hyper;
            loop {
                let (stream, remote_addr) = self.listener.accept().await?;
                let tower_service = self.service.clone();
                let enable_connect_info = self.enable_connect_info;
                let configure_hyper = configure_hyper.clone();

                tokio::task::spawn(async move {
                    let io = hyper_util::rt::TokioIo::new(stream);
                    let hyper_service = hyper::service::service_fn(move |mut request| {
                        if enable_connect_info {
                            request.extensions_mut().insert(crate::request::connect_info::ConnectInfo(remote_addr));
                        }
                        tower_service.clone().call(request)
                    });

                    let mut builder = hyper_util::server::conn::auto::Builder::new(
                        hyper_util::rt::TokioExecutor::new(),
                    );
                    if let Some(configure) = configure_hyper {
                        builder = configure(builder);
                    }

                    if let Err(err) = builder.serve_connection_with_upgrades(io, hyper_service).await {
                        tracing::error!(error = ?err, "error serving connection");
                    }
                });
            }
        })
    }
}

/// A server with graceful shutdown enabled.
///
/// Created by [`Serve::with_graceful_shutdown`].
pub struct WithGracefulShutdown<S, F> {
    listener: tokio::net::TcpListener,
    service: S,
    signal: F,
    enable_connect_info: bool,
    configure_hyper: Option<ConfigureHyperFn>,
    shutdown_timeout: Option<Duration>,
}

impl<S, F> WithGracefulShutdown<S, F> {
    fn new(
        listener: tokio::net::TcpListener,
        service: S,
        signal: F,
        enable_connect_info: bool,
        configure_hyper: Option<ConfigureHyperFn>,
    ) -> Self
    where
        F: Future<Output = ()> + Send + 'static,
    {
        Self {
            listener,
            service,
            signal,
            enable_connect_info,
            configure_hyper,
            shutdown_timeout: None,
        }
    }

    /// Enable `ConnectInfo` extraction in request handlers.
    ///
    /// When enabled, the remote address will be available in handlers via
    /// `ConnectInfo<SocketAddr>` extraction.
    pub fn with_connect_info(mut self) -> Self {
        self.enable_connect_info = true;
        self
    }

    /// Configure the underlying Hyper server connection builder.
    ///
    /// This allows customization of Hyper settings like:
    /// - `http1_max_buf_size()`
    /// - `http2_max_concurrent_streams()`
    /// - Other protocol-specific settings
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use tokio::net::TcpListener;
    ///
    /// let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    /// aws_smithy_http_server::serve(listener, app)
    ///     .with_graceful_shutdown(shutdown_signal())
    ///     .configure_hyper(|builder| {
    ///         builder
    ///             .http1_max_buf_size(16 * 1024)
    ///             .http2_max_concurrent_streams(200)
    ///     })
    ///     .await
    ///     .unwrap();
    /// ```
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
    ///
    /// If the timeout expires before all in-flight connections finish,
    /// the server will stop waiting and return. This only applies to the
    /// shutdown phase, not the normal server operation.
    ///
    /// By default, there is no timeout (waits indefinitely).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::time::Duration;
    /// use tokio::net::TcpListener;
    ///
    /// let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    /// aws_smithy_http_server::serve(listener, app)
    ///     .with_graceful_shutdown(shutdown_signal())
    ///     .with_shutdown_timeout(Duration::from_secs(30))
    ///     .await
    ///     .unwrap();
    /// ```
    pub fn with_shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.shutdown_timeout = Some(timeout);
        self
    }
}

// Implement IntoFuture so we can await WithGracefulShutdown directly
impl<S, F> std::future::IntoFuture for WithGracefulShutdown<S, F>
where
    S: Service<http::Request<hyper::body::Incoming>, Response = http::Response<crate::body::BoxBody>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send,
    F: Future<Output = ()> + Send + 'static,
{
    type Output = io::Result<()>;
    type IntoFuture = Pin<Box<dyn Future<Output = io::Result<()>> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            // Create watch channels for shutdown coordination (Axum pattern)
            let (shutdown_tx, shutdown_rx) = watch::channel(());
            let (inflight_tx, mut inflight_rx) = watch::channel(());

            // Spawn the shutdown signal watcher
            tokio::spawn(async move {
                self.signal.await;
                drop(shutdown_rx); // This triggers shutdown_tx.closed()
            });

            let enable_connect_info = self.enable_connect_info;
            let configure_hyper = self.configure_hyper;
            let shutdown_timeout = self.shutdown_timeout;

            loop {
                let (stream, remote_addr) = tokio::select! {
                    // Shutdown signal received - stop accepting new connections
                    _ = shutdown_tx.closed() => {
                        break;
                    }
                    // Accept new connection
                    result = self.listener.accept() => {
                        result?
                    }
                };

                let tower_service = self.service.clone();
                let inflight_tx = inflight_tx.clone();
                let configure_hyper = configure_hyper.clone();

                tokio::task::spawn(async move {
                    let io = hyper_util::rt::TokioIo::new(stream);
                    let hyper_service = hyper::service::service_fn(move |mut request| {
                        if enable_connect_info {
                            request.extensions_mut().insert(crate::request::connect_info::ConnectInfo(remote_addr));
                        }
                        tower_service.clone().call(request)
                    });

                    let mut builder = hyper_util::server::conn::auto::Builder::new(
                        hyper_util::rt::TokioExecutor::new(),
                    );
                    if let Some(configure) = configure_hyper {
                        builder = configure(builder);
                    }

                    if let Err(err) = builder.serve_connection_with_upgrades(io, hyper_service).await {
                        tracing::error!(error = ?err, "error serving connection");
                    }

                    // Connection is done - drop inflight_tx
                    drop(inflight_tx);
                });
            }

            // Dropped out of the loop - shutdown signal received
            // Drop the original inflight_tx so inflight_rx will notice when all connections finish
            drop(inflight_tx);

            // Wait for all in-flight connections to finish (with optional timeout)
            // When all inflight_tx clones are dropped, the receiver will see the channel is closed
            match shutdown_timeout {
                Some(timeout) => {
                    match tokio::time::timeout(timeout, inflight_rx.changed()).await {
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
                    let _ = inflight_rx.changed().await;
                    tracing::debug!("all in-flight connections completed during graceful shutdown");
                }
            }

            Ok(())
        })
    }
}
