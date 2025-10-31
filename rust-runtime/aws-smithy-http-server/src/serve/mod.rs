/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Serve utilities for running HTTP servers.
//!
//! This module provides a convenient [`serve`] function similar to `axum::serve`
//! for easily serving Tower services with Hyper.
//!
//! ## When to Use This Module
//!
//! - Use [`serve`] when you need a simple, batteries-included HTTP server
//! - For more control over the Hyper connection builder, use [`.configure_hyper()`](Serve::configure_hyper)
//! - For Lambda environments, see the `aws-lambda` feature and `routing::lambda_handler`
//!
//! ## How It Works
//!
//! The `serve` function creates a connection acceptance loop that:
//!
//! 1. **Accepts connections** via the [`Listener`] trait (e.g., [`TcpListener`](tokio::net::TcpListener))
//! 2. **Creates per-connection services** by calling your `make_service` with [`IncomingStream`]
//! 3. **Converts Tower services to Hyper** using `TowerToHyperService`
//! 4. **Spawns a task** for each connection to handle HTTP requests
//!
//! ```text
//! ┌─────────┐      ┌──────────────┐      ┌──────────────┐      ┌────────┐
//! │Listener │─────▶│IncomingStream│─────▶│ make_service │─────▶│ Hyper  │
//! │ accept  │      │ (io + addr)  │      │  (Tower)     │      │ spawn  │
//! └─────────┘      └──────────────┘      └──────────────┘      └────────┘
//! ```
//!
//! The [`IncomingStream`] provides connection metadata to your service factory,
//! allowing per-connection customization based on remote address or IO type
//!
//! ## HTTP Protocol Selection
//!
//! By default, `serve` uses HTTP/1 with upgrade support, allowing clients to
//! negotiate HTTP/2 via the HTTP/1.1 Upgrade mechanism or ALPN. The protocol is
//! auto-detected for each connection.
//!
//! You can customize this behavior with [`.configure_hyper()`](Serve::configure_hyper):
//!
//! ```rust,ignore
//! // Force HTTP/2 only (skips upgrade negotiation)
//! serve(listener, app.into_make_service())
//!     .configure_hyper(|builder| {
//!         builder.http2_only()
//!     })
//!     .await?;
//!
//! // Force HTTP/1 only with keep-alive
//! serve(listener, app.into_make_service())
//!     .configure_hyper(|builder| {
//!         builder.http1().keep_alive(true)
//!     })
//!     .await?;
//! ```
//!
//! **Performance note**: When using `.http2_only()` or `.http1()`, the server skips
//! the HTTP/1 upgrade preface reading, which can reduce connection setup latency.
//!
//! ## Graceful Shutdown
//!
//! Graceful shutdown is zero-cost when not used - no watch channels are allocated
//! and no `tokio::select!` overhead is incurred. Call
//! [`.with_graceful_shutdown(signal)`](Serve::with_graceful_shutdown) to enable it:
//!
//! ```ignore
//! serve(listener, service)
//!     .with_graceful_shutdown(async {
//!         tokio::signal::ctrl_c().await.expect("failed to listen for Ctrl+C");
//!     })
//!     .await
//! ```
//!
//! This ensures in-flight requests complete before shutdown. Use
//! [`.with_shutdown_timeout(duration)`](ServeWithGracefulShutdown::with_shutdown_timeout)
//! to set a maximum wait time.
//!
//! ## Common Patterns
//!
//! ### Limiting Concurrent Connections
//!
//! Use [`ListenerExt::limit_connections`] to prevent resource exhaustion:
//!
//! ```rust,ignore
//! use aws_smithy_http_server::serve::ListenerExt;
//!
//! let listener = TcpListener::bind("0.0.0.0:3000")
//!     .await?
//!     .limit_connections(1000);  // Max 1000 concurrent connections
//!
//! serve(listener, app.into_make_service()).await?;
//! ```
//!
//! ### Accessing Connection Information
//!
//! Use `.into_make_service_with_connect_info::<T>()` to access connection metadata
//! in your handlers:
//!
//! ```rust,ignore
//! use std::net::SocketAddr;
//! use aws_smithy_http_server::request::connect_info::ConnectInfo;
//!
//! // In your handler:
//! async fn my_handler(ConnectInfo(addr): ConnectInfo<SocketAddr>) -> String {
//!     format!("Request from: {}", addr)
//! }
//!
//! // When serving:
//! serve(
//!     listener,
//!     app.into_make_service_with_connect_info::<SocketAddr>()
//! ).await?;
//! ```
//!
//! ### Custom TCP Settings
//!
//! Use [`ListenerExt::tap_io`] to configure TCP options:
//!
//! ```rust,ignore
//! use aws_smithy_http_server::serve::ListenerExt;
//!
//! let listener = TcpListener::bind("0.0.0.0:3000")
//!     .await?
//!     .tap_io(|stream| {
//!         let _ = stream.set_nodelay(true);
//!     });
//!
//! serve(listener, app.into_make_service()).await?;
//! ```
//!
//! ## Troubleshooting
//!
//! ### Type Errors
//!
//! If you encounter complex error messages about trait bounds, check:
//!
//! 1. **Service Error Type**: Your service must have `Error = Infallible`
//!    ```rust,ignore
//!    // ✓ Correct - handlers return responses, not Results
//!    async fn handler() -> Response<Body> { ... }
//!
//!    // ✗ Wrong - cannot use Result<Response, E>
//!    async fn handler() -> Result<Response<Body>, MyError> { ... }
//!    ```
//!
//! 2. **MakeService Wrapper**: Use the correct wrapper for your service:
//!    ```rust,ignore
//!    use aws_smithy_http_server::routing::IntoMakeService;
//!
//!    // For Smithy services:
//!    app.into_make_service()
//!
//!    // For services with middleware:
//!    IntoMakeService::new(service)
//!    ```
//!
//! ### Graceful Shutdown Not Working
//!
//! If graceful shutdown doesn't wait for connections:
//!
//! - Ensure you call `.with_graceful_shutdown()` **before** `.await`
//! - The signal future must be `Send + 'static`
//! - Consider adding a timeout with `.with_shutdown_timeout()`
//!
//! ### Connection Limit Not Applied
//!
//! Remember that `.limit_connections()` applies to the listener **before** passing
//! it to `serve()`:
//!
//! ```rust,ignore
//! // ✓ Correct
//! let listener = TcpListener::bind("0.0.0.0:3000")
//!     .await?
//!     .limit_connections(100);
//! serve(listener, app.into_make_service()).await?;
//!
//! // ✗ Wrong - limit_connections must be called on listener
//! serve(TcpListener::bind("0.0.0.0:3000").await?, app.into_make_service())
//!     .limit_connections(100)  // This method doesn't exist on Serve
//!     .await?;
//! ```
//!
//! ## Advanced: Custom Connection Handling
//!
//! If you need per-connection customization (e.g., different Hyper settings based on
//! the remote address), you can implement your own connection loop using the building
//! blocks provided by this module:
//!
//! ```rust,ignore
//! use aws_smithy_http_server::routing::IntoMakeService;
//! use aws_smithy_http_server::serve::Listener;
//! use hyper_util::rt::{TokioExecutor, TokioIo};
//! use hyper_util::server::conn::auto::Builder;
//! use hyper_util::service::TowerToHyperService;
//! use tower::ServiceExt;
//!
//! let mut listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
//! let make_service = app.into_make_service_with_connect_info::<SocketAddr>();
//!
//! loop {
//!     let (stream, remote_addr) = listener.accept().await?;
//!     let io = TokioIo::new(stream);
//!
//!     // Per-connection Hyper configuration
//!     let mut builder = Builder::new(TokioExecutor::new());
//!     if remote_addr.ip().is_loopback() {
//!         builder = builder.http2_only();  // Local connections use HTTP/2
//!     } else {
//!         builder = builder.http1().keep_alive(true);  // External use HTTP/1
//!     }
//!
//!     let tower_service = make_service
//!         .ready()
//!         .await?
//!         .call(IncomingStream { io: &io, remote_addr })
//!         .await?;
//!
//!     let hyper_service = TowerToHyperService::new(tower_service);
//!
//!     tokio::spawn(async move {
//!         if let Err(err) = builder.serve_connection(io, hyper_service).await {
//!             eprintln!("Error serving connection: {}", err);
//!         }
//!     });
//! }
//! ```
//!
//! This approach provides complete flexibility while still leveraging the efficient
//! Hyper and Tower integration provided by this module.
//!
//! Portions of the implementation are adapted from axum
//! (<https://github.com/tokio-rs/axum>), which is licensed under the MIT License.
//! Copyright (c) 2019 Axum Contributors

use std::convert::Infallible;
use std::error::Error as StdError;
use std::fmt::{self, Debug};
use std::future::{Future, IntoFuture};
use std::io;
use std::marker::PhantomData;
use std::pin::Pin;
use std::time::Duration;

use http_body::Body as HttpBody;
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use hyper_util::server::graceful::GracefulShutdown;
use hyper_util::service::TowerToHyperService;
use tower::{Service, ServiceExt as _};

mod listener;

pub use self::listener::{ConnLimiter, ConnLimiterIo, Listener, ListenerExt, TapIo};

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
/// # Lifetime Safety
///
/// The lifetime `'a` ensures the reference to IO remains valid only during the
/// `make_service.call()` invocation. After your service is created, the IO is
/// moved into a spawned task to handle the connection. This is safe because:
///
/// ```text
/// let io = TokioIo::new(stream);           // IO created
/// let service = make_service.call(
///     IncomingStream { io: &io, .. }       // Borrowed during call
/// ).await;                                  // Borrow ends
/// tokio::spawn(serve_connection(io, ..));  // IO moved to task
/// ```
///
/// The borrow checker guarantees the reference doesn't outlive the IO object.
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
/// This function accepts services wrapped with [`crate::routing::IntoMakeService`] or
/// [`crate::routing::IntoMakeServiceWithConnectInfo`].
///
/// For generated Smithy services, use `.into_make_service()` or
/// `.into_make_service_with_connect_info::<C>()`. For services wrapped with
/// Tower middleware, use `IntoMakeService::new(service)`.
///
/// # Error Handling
///
/// Note that both `make_service` and the generated service must have `Error = Infallible`.
/// This means:
/// - Your service factory cannot fail when creating per-connection services
/// - Your request handlers cannot return errors (use proper HTTP error responses instead)
///
/// If you need fallible service creation, consider handling errors within your
/// `make_service` implementation and returning a service that produces error responses.
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

/// A server future that serves HTTP connections.
///
/// This is the return type of [`serve`]. It implements [`IntoFuture`], so
/// you can directly `.await` it:
///
/// ```ignore
/// serve(listener, service).await?;
/// ```
///
/// Before awaiting, you can configure it:
/// - [`configure_hyper`](Self::configure_hyper) - Configure Hyper's connection builder
/// - [`with_graceful_shutdown`](Self::with_graceful_shutdown) - Enable graceful shutdown
/// - [`local_addr`](Self::local_addr) - Get the bound address
///
/// Created by [`serve`].
#[must_use = "Serve does nothing until you `.await` or call `.into_future()` on it"]
pub struct Serve<L, M, S, B> {
    listener: L,
    make_service: M,
    hyper_builder: Option<Builder<TokioExecutor>>,
    _marker: PhantomData<(S, B)>,
}

impl<L, M, S, B> fmt::Debug for Serve<L, M, S, B>
where
    L: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Serve")
            .field("listener", &self.listener)
            .field("has_hyper_config", &self.hyper_builder.is_some())
            .finish_non_exhaustive()
    }
}

impl<L, M, S, B> Serve<L, M, S, B>
where
    L: Listener,
{
    fn new(listener: L, make_service: M) -> Self {
        Self {
            listener,
            make_service,
            hyper_builder: None,
            _marker: PhantomData,
        }
    }

    /// Configure the underlying Hyper connection builder.
    ///
    /// This allows you to customize Hyper's HTTP/1 and HTTP/2 settings,
    /// such as timeouts, max concurrent streams, keep-alive behavior, etc.
    ///
    /// The configuration is applied once and the configured builder is cloned
    /// for each connection, providing optimal performance.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use std::time::Duration;
    ///
    /// serve(listener, service)
    ///     .configure_hyper(|builder| {
    ///         builder
    ///             .http1()
    ///             .keep_alive(true)
    ///             .http2()
    ///             .max_concurrent_streams(200)
    ///     })
    ///     .await?;
    /// ```
    ///
    /// # Advanced: Per-Connection Configuration
    ///
    /// If you need per-connection customization (e.g., different settings based on
    /// the remote address), you can implement your own connection loop. See the
    /// module-level documentation for examples.
    pub fn configure_hyper<F>(mut self, f: F) -> Self
    where
        F: FnOnce(
            hyper_util::server::conn::auto::Builder<hyper_util::rt::TokioExecutor>,
        ) -> hyper_util::server::conn::auto::Builder<hyper_util::rt::TokioExecutor>,
    {
        let builder = Builder::new(TokioExecutor::new());
        self.hyper_builder = Some(f(builder));
        self
    }

    /// Enable graceful shutdown for the server.
    pub fn with_graceful_shutdown<F>(self, signal: F) -> ServeWithGracefulShutdown<L, M, S, F, B>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        ServeWithGracefulShutdown::new(self.listener, self.make_service, signal, self.hyper_builder)
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
        // Decide once at serve-time which path to use based on the configured builder
        let use_upgrades = self
            .hyper_builder
            .as_ref()
            .map(|b| b.is_http1_available() && b.is_http2_available())
            .unwrap_or(true); // Default to auto-detect if no builder configured

        Box::pin(async move {
            let Self {
                mut listener,
                mut make_service,
                hyper_builder,
                _marker,
            } = self;

            loop {
                let (io, remote_addr) = listener.accept().await;
                handle_connection(
                    &mut make_service,
                    io,
                    remote_addr,
                    hyper_builder.as_ref(),
                    use_upgrades,
                    None,
                )
                .await;
            }
        })
    }
}

/// A server future with graceful shutdown enabled.
///
/// This type is created by calling [`Serve::with_graceful_shutdown`]. It implements
/// [`IntoFuture`], so you can directly `.await` it.
///
/// When the shutdown signal completes, the server will:
/// 1. Stop accepting new connections
/// 2. Wait for all in-flight requests to complete (or until timeout if configured)
/// 3. Return once all connections are closed
///
/// Configure the shutdown timeout with [`with_shutdown_timeout`](Self::with_shutdown_timeout).
///
/// Created by [`Serve::with_graceful_shutdown`].
#[must_use = "ServeWithGracefulShutdown does nothing until you `.await` or call `.into_future()` on it"]
pub struct ServeWithGracefulShutdown<L, M, S, F, B> {
    listener: L,
    make_service: M,
    signal: F,
    hyper_builder: Option<Builder<TokioExecutor>>,
    shutdown_timeout: Option<Duration>,
    _marker: PhantomData<(S, B)>,
}

impl<L, M, S, F, B> fmt::Debug for ServeWithGracefulShutdown<L, M, S, F, B>
where
    L: Listener + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServeWithGracefulShutdown")
            .field("listener", &self.listener)
            .field("has_hyper_config", &self.hyper_builder.is_some())
            .field("shutdown_timeout", &self.shutdown_timeout)
            .finish_non_exhaustive()
    }
}

impl<L: Listener, M, S, F, B> ServeWithGracefulShutdown<L, M, S, F, B> {
    fn new(listener: L, make_service: M, signal: F, hyper_builder: Option<Builder<TokioExecutor>>) -> Self
    where
        F: Future<Output = ()> + Send + 'static,
    {
        Self {
            listener,
            make_service,
            signal,
            hyper_builder,
            shutdown_timeout: None,
            _marker: PhantomData,
        }
    }

    /// Set a timeout for graceful shutdown.
    ///
    /// If the timeout expires before all connections complete, a warning is logged
    /// and the server returns successfully. Note that this does **not** forcibly
    /// terminate connections - it only stops waiting for them.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::time::Duration;
    ///
    /// serve(listener, app.into_make_service())
    ///     .with_graceful_shutdown(shutdown_signal())
    ///     .with_shutdown_timeout(Duration::from_secs(30))
    ///     .await?;  // Returns Ok(()) even if timeout expires
    /// ```
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
impl<L, M, S, F, B> IntoFuture for ServeWithGracefulShutdown<L, M, S, F, B>
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
        // Decide once at serve-time which path to use based on the configured builder
        let use_upgrades = self
            .hyper_builder
            .as_ref()
            .map(|b| b.is_http1_available() && b.is_http2_available())
            .unwrap_or(true); // Default to auto-detect if no builder configured

        Box::pin(async move {
            let Self {
                mut listener,
                mut make_service,
                signal,
                hyper_builder,
                shutdown_timeout,
                _marker,
            } = self;

            // Create graceful shutdown coordinator
            let graceful = GracefulShutdown::new();
            let mut signal = std::pin::pin!(signal);

            loop {
                tokio::select! {
                    result = listener.accept() => {
                        let (io, remote_addr) = result;

                        handle_connection(
                            &mut make_service,
                            io,
                            remote_addr,
                            hyper_builder.as_ref(),
                            use_upgrades,
                            Some(&graceful),
                        )
                        .await;
                    }
                    _ = signal.as_mut() => {
                        tracing::debug!("received graceful shutdown signal, not accepting new connections");
                        break;
                    }
                }
            }

            drop(listener);

            tracing::debug!("waiting for {} task(s) to finish", graceful.count());

            // Wait for all in-flight connections to finish (with optional timeout)
            match shutdown_timeout {
                Some(timeout) => match tokio::time::timeout(timeout, graceful.shutdown()).await {
                    Ok(_) => {
                        tracing::debug!("all in-flight connections completed during graceful shutdown");
                    }
                    Err(_) => {
                        tracing::warn!(
                            timeout_secs = timeout.as_secs(),
                            "graceful shutdown timeout expired, some connections may not have completed"
                        );
                    }
                },
                None => {
                    graceful.shutdown().await;
                    tracing::debug!("all in-flight connections completed during graceful shutdown");
                }
            }

            Ok(())
        })
    }
}

async fn handle_connection<L, M, S, B>(
    make_service: &mut M,
    conn_io: <L as Listener>::Io,
    remote_addr: <L as Listener>::Addr,
    hyper_builder: Option<&Builder<TokioExecutor>>,
    use_upgrades: bool,
    graceful: Option<&GracefulShutdown>,
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
    let watcher = graceful.map(|g| g.watcher());
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

    let builder = hyper_builder
        .cloned()
        .unwrap_or_else(|| Builder::new(TokioExecutor::new()));

    tokio::spawn(async move {
        let result = if use_upgrades {
            // Auto-detect mode - use with_upgrades for HTTP/1 upgrade support
            let conn = builder.serve_connection_with_upgrades(tokio_io, hyper_service);
            if let Some(watcher) = watcher {
                watcher.watch(conn).await
            } else {
                conn.await
            }
        } else {
            // Protocol is already decided (http1_only or http2_only) - skip preface reading
            let conn = builder.serve_connection(tokio_io, hyper_service);
            if let Some(watcher) = watcher {
                watcher.watch(conn).await
            } else {
                conn.await
            }
        };

        if let Err(err) = result {
            tracing::error!(error = ?err, "error serving connection");
        }
    });
}
