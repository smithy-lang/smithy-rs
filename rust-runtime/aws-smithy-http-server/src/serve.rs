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
use std::task::{Context, Poll};

use pin_project_lite::pin_project;
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
    pub fn with_graceful_shutdown<F>(self, signal: F) -> WithGracefulShutdown<S, F>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        WithGracefulShutdown::new(self.listener, self.service, signal)
    }
}

pin_project! {
    /// Future for [`Serve`].
    pub struct ServeFuture<S> {
        listener: tokio::net::TcpListener,
        service: S,
    }
}

impl<S> Future for ServeFuture<S>
where
    S: Service<http::Request<hyper::body::Incoming>, Response = http::Response<crate::body::BoxBody>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send,
{
    type Output = io::Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        loop {
            match this.listener.poll_accept(cx) {
                Poll::Ready(Ok((stream, _remote_addr))) => {
                    let tower_service = this.service.clone();

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
                    // Continue loop to accept more connections
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            }
        }
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
    type IntoFuture = ServeFuture<S>;

    fn into_future(self) -> Self::IntoFuture {
        ServeFuture {
            listener: self.listener,
            service: self.service,
        }
    }
}

/// A server with graceful shutdown enabled.
///
/// Created by [`Serve::with_graceful_shutdown`].
pub struct WithGracefulShutdown<S, F> {
    listener: tokio::net::TcpListener,
    service: S,
    signal: F,
}

impl<S, F> WithGracefulShutdown<S, F> {
    fn new(listener: tokio::net::TcpListener, service: S, signal: F) -> Self
    where
        F: Future<Output = ()> + Send + 'static,
    {
        Self {
            listener,
            service,
            signal,
        }
    }
}

pin_project! {
    /// Future for [`WithGracefulShutdown`].
    pub struct WithGracefulShutdownFuture<S> {
        listener: tokio::net::TcpListener,
        service: S,
        signal_tx: watch::Sender<()>,
        close_tx: Option<watch::Sender<()>>,
        close_rx: watch::Receiver<()>,
        state: ShutdownState,
    }
}

enum ShutdownState {
    /// Accepting new connections
    Running,
    /// Shutdown signal received, draining connections
    Draining,
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
    type IntoFuture = WithGracefulShutdownFuture<S>;

    fn into_future(self) -> Self::IntoFuture {
        // Create watch channels for shutdown coordination (Axum pattern)
        let (signal_tx, signal_rx) = watch::channel(());
        let (close_tx, close_rx) = watch::channel(());

        // Spawn the shutdown signal watcher
        tokio::spawn(async move {
            self.signal.await;
            drop(signal_rx); // This triggers signal_tx.closed()
        });

        WithGracefulShutdownFuture {
            listener: self.listener,
            service: self.service,
            signal_tx,
            close_tx: Some(close_tx),
            close_rx,
            state: ShutdownState::Running,
        }
    }
}

impl<S> Future for WithGracefulShutdownFuture<S>
where
    S: Service<http::Request<hyper::body::Incoming>, Response = http::Response<crate::body::BoxBody>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send,
{
    type Output = io::Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        loop {
            match this.state {
                ShutdownState::Running => {
                    // Check if shutdown signal received
                    if this.signal_tx.is_closed() {
                        // Stop accepting new connections, start draining
                        *this.state = ShutdownState::Draining;
                        // Drop the original close_tx
                        this.close_tx.take();
                        continue;
                    }

                    // Try to accept a new connection
                    match this.listener.poll_accept(cx) {
                        Poll::Ready(Ok((stream, _remote_addr))) => {
                            let tower_service = this.service.clone();
                            let close_tx = this.close_tx.as_ref().expect("close_tx should exist in Running state").clone();

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
                            // Continue loop to accept more connections
                        }
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                        Poll::Pending => return Poll::Pending,
                    }
                }
                ShutdownState::Draining => {
                    // Wait for all connections to complete
                    // Pin the future on the stack
                    let mut drain_fut = std::pin::pin!(this.close_rx.changed());
                    match drain_fut.as_mut().poll(cx) {
                        Poll::Ready(Ok(())) => {
                            // Value changed (shouldn't happen, but connections might still be running)
                            // Continue polling
                            cx.waker().wake_by_ref();
                            return Poll::Pending;
                        }
                        Poll::Ready(Err(_)) => {
                            // Channel closed - all connection tasks dropped their senders
                            return Poll::Ready(Ok(()));
                        }
                        Poll::Pending => {
                            // Still waiting for connections to complete
                            return Poll::Pending;
                        }
                    }
                }
            }
        }
    }
}
