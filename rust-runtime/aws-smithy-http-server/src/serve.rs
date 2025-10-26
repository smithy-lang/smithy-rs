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

use tokio::sync::watch;
use tower::Service;

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
}

impl<S> Serve<S> {
    fn new(listener: tokio::net::TcpListener, service: S) -> Self {
        Self { listener, service }
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
    pub fn with_graceful_shutdown<F>(self, signal: F) -> WithGracefulShutdown<S>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        WithGracefulShutdown::new(self.listener, self.service, signal)
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
            loop {
                let (stream, _remote_addr) = self.listener.accept().await?;
                let tower_service = self.service.clone();

                tokio::task::spawn(async move {
                    let io = hyper_util::rt::TokioIo::new(stream);
                    let hyper_service = hyper::service::service_fn(move |request| {
                        tower_service.clone().call(request)
                    });

                    if let Err(err) = hyper_util::server::conn::auto::Builder::new(
                        hyper_util::rt::TokioExecutor::new(),
                    )
                    .serve_connection_with_upgrades(io, hyper_service)
                    .await
                    {
                        eprintln!("Error serving connection: {:?}", err);
                    }
                });
            }
        })
    }
}

/// A server with graceful shutdown enabled.
///
/// Created by [`Serve::with_graceful_shutdown`].
pub struct WithGracefulShutdown<S> {
    listener: tokio::net::TcpListener,
    service: S,
    signal: Pin<Box<dyn Future<Output = ()> + Send>>,
}

impl<S> WithGracefulShutdown<S> {
    fn new<F>(listener: tokio::net::TcpListener, service: S, signal: F) -> Self
    where
        F: Future<Output = ()> + Send + 'static,
    {
        Self {
            listener,
            service,
            signal: Box::pin(signal),
        }
    }
}

// Implement IntoFuture so we can await WithGracefulShutdown directly
impl<S> std::future::IntoFuture for WithGracefulShutdown<S>
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
            // Create a watch channel for the shutdown signal
            // Based on axum's graceful shutdown implementation
            let (signal_tx, signal_rx) = watch::channel(());
            let (close_tx, mut close_rx) = watch::channel(());

            // Spawn the shutdown signal watcher
            tokio::spawn(async move {
                self.signal.await;
                drop(signal_rx);
            });

            loop {
                let (stream, _remote_addr) = tokio::select! {
                    // Shutdown signal received - stop accepting new connections
                    _ = signal_tx.closed() => {
                        break;
                    }
                    // Accept new connection
                    result = self.listener.accept() => {
                        result?
                    }
                };

                let tower_service = self.service.clone();
                let close_tx = close_tx.clone();

                tokio::task::spawn(async move {
                    let io = hyper_util::rt::TokioIo::new(stream);
                    let hyper_service = hyper::service::service_fn(move |request| {
                        tower_service.clone().call(request)
                    });

                    if let Err(err) = hyper_util::server::conn::auto::Builder::new(
                        hyper_util::rt::TokioExecutor::new(),
                    )
                    .serve_connection_with_upgrades(io, hyper_service)
                    .await
                    {
                        eprintln!("Error serving connection: {:?}", err);
                    }

                    // Connection is done - drop close_tx
                    drop(close_tx);
                });
            }

            // Dropped out of the loop - signal was received
            // Drop the original close_tx so close_rx will notice when all connections finish
            drop(close_tx);

            // Wait for all active connections to finish
            // When all close_tx clones are dropped, the receiver will see the channel is closed
            let _ = close_rx.changed().await;

            Ok(())
        })
    }
}
