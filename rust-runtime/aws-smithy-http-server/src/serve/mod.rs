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
//! 2. **Creates per-connection services** by calling the `make_service` with [`IncomingStream`]
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
//! The [`IncomingStream`] provides connection metadata to the service factory,
//! allowing per-connection customization based on remote address or IO type
//!
//! ## HTTP Protocol Selection
//!
//! By default, `serve` uses Hyper's automatic HTTP/1 or HTTP/2 selection with
//! HTTP upgrade support when both protocols are enabled. Customize settings with
//! [`.configure_hyper()`](Serve::configure_hyper).
//!
//! Set `builder.http1_only()` or `builder.http2_only()` in `configure_hyper`
//! to serve only that protocol. Other protocols are rejected by Hyper before
//! reaching the request service. Without either setting, both protocols are
//! automatically detected.
//!
//! When both protocols are enabled, `serve` uses Hyper's upgrade-capable API.
//! With `http1_only()` or `http2_only()`, it uses the regular connection API to
//! preserve the protocol restriction. HTTP/1-only upgrades require a custom
//! serve loop using Hyper's native HTTP/1 builder.
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
//! `serve` limits concurrently accepted connections to 16,384 by default. Configure
//! or disable the limit on the server future:
//!
//! ```rust,ignore
//! serve(listener, app.into_make_service())
//!     .max_connections(1000)
//!     .await?;
//! ```
//!
//! [`ListenerExt::limit_connections`] remains available for listener-level composition.
//! The connection permit stays attached to the socket across HTTP upgrades.
//! Applications that downcast upgraded IO with
//! `hyper_util::server::conn::auto::upgrade::downcast` must use
//! `TokioIo<ConnLimiterIo<L::Io>>` as the IO type. [`IncomingStream`] continues
//! to expose the listener's original IO type to the service factory.
//!
//! A 30-second header-read deadline starts when the connection task begins driving
//! Hyper and ends when Hyper delivers the first complete request head to its service.
//! It includes protocol negotiation and parsing for either HTTP version. Connection
//! service creation, Tower readiness, request middleware, and body reads are excluded.
//! Configure it with [`Serve::header_read_timeout`]. The same setting is applied to
//! Hyper's HTTP/1 header-read timeout for subsequent requests. **Header-read timeout
//! protection for every request is currently supported only for HTTP/1.** For HTTP/2,
//! only the first request head on the connection is covered; later streams have no
//! header-read deadline. HTTP/2 keep-alive pings do not provide that protection.
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
//! let listener = TcpListener::bind("127.0.0.1:3000")
//!     .await?
//!     .tap_io(|stream| {
//!         let _ = stream.set_nodelay(true);
//!     });
//!
//! serve(listener, app.into_make_service()).await?;
//! ```
//!
//! ## Timeouts and Connection Management
//!
//! ### Available Timeout Types
//!
//! | Timeout Type | What It Does | How to Configure |
//! |--------------|--------------|------------------|
//! | **Header Read** | Per-request protection currently supports HTTP/1 only; HTTP/2 covers only the connection's first request head | 30 s by default; override via [`.header_read_timeout()`](Serve::header_read_timeout) |
//! | **Request** | Time limit for processing one request | Tower's `TimeoutLayer` |
//! | **Connection Duration** | Total connection lifetime limit | Custom accept loop with `tokio::time::timeout` |
//! | **HTTP/2 Keep-Alive** | Idle timeout between HTTP/2 requests | 20 s ping interval by default; override via `.configure_hyper()` with `.http2().keep_alive_*()` |
//!
//! **Examples:**
//! - `examples/header_read_timeout.rs` - Configure header read timeout
//! - `examples/request_timeout.rs` - Add request-level timeouts
//! - `examples/custom_accept_loop.rs` - Implement connection duration limits
//! - `examples/http2_keepalive.rs` - Configure HTTP/2 keep-alive
//! - `examples/connection_limiting.rs` - Limit concurrent connections
//! - `examples/request_concurrency_limiting.rs` - Limit concurrent requests
//!
//! ### Connection Duration vs Idle Timeout
//!
//! **Connection duration timeout**: Closes the connection after N seconds total, regardless of activity.
//! Implemented with `tokio::time::timeout` wrapping the connection future.
//!
//! **Idle timeout**: Closes the connection only when inactive between requests.
//! - HTTP/2: Available via `.keep_alive_interval()` and `.keep_alive_timeout()`
//! - HTTP/1.1: Not available without modifying Hyper
//!
//! See `examples/custom_accept_loop.rs` for a working connection duration timeout example.
//!
//! ### Connection Limiting vs Request Limiting
//!
//! **Connection limiting** (`.limit_connections()`): Limits the number of TCP connections.
//! Use this to prevent socket/file descriptor exhaustion.
//!
//! **Request limiting** (`ConcurrencyLimitLayer`): Limits in-flight requests.
//! Use this to prevent work queue exhaustion. With HTTP/2, one connection can have multiple
//! requests in flight simultaneously.
//!
//! Most applications should use both - they protect different layers.
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
//! let listener = TcpListener::bind("127.0.0.1:3000")
//!     .await?
//!     .limit_connections(100);
//! serve(listener, app.into_make_service()).await?;
//!
//! // ✗ Wrong - limit_connections must be called on listener
//! serve(TcpListener::bind("127.0.0.1:3000").await?, app.into_make_service())
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
//! let mut listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
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
use std::sync::Arc;
use std::time::Duration;

use http_body::Body as HttpBody;
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto::Builder;
use hyper_util::server::graceful::{GracefulConnection, GracefulShutdown, Watcher};
use hyper_util::service::TowerToHyperService;
use tokio::sync::OwnedSemaphorePermit;
use tower::{Service, ServiceExt as _};

mod header_read_timeout;
mod listener;

use self::header_read_timeout::{HeaderReadDeadline, HeaderReadTimeoutService};

use self::listener::ConnectionLimit;
pub use self::listener::{ConnLimiter, ConnLimiterIo, Listener, ListenerExt, TapIo};

use crate::settings::{default_connection_builder, ServeSettings};
pub use crate::settings::{
    DEFAULT_HEADER_READ_TIMEOUT, DEFAULT_HTTP2_KEEP_ALIVE_INTERVAL, DEFAULT_HTTP2_KEEP_ALIVE_TIMEOUT,
    DEFAULT_MAX_CONNECTIONS, DEFAULT_SHUTDOWN_TIMEOUT,
};

// ============================================================================
// Type Bounds Documentation
// ============================================================================
//
// ## Body Bounds (B)
// HTTP response bodies must satisfy:
// - `B: HttpBody + Send + 'static` - Implement the body trait and be sendable
// - `B::Data: Send` - Data chunks must be sendable across threads
// - `B::Error: Into<Box<dyn StdError + Send + Sync>>` - Errors must be convertible
//
// ## Service Bounds (S)
//
// The `S` type parameter represents a **per-connection HTTP service** - a Tower service
// that handles individual HTTP requests and returns HTTP responses.
//
// Required bounds:
// - `S: Service<http::Request<Incoming>, Response = http::Response<B>, Error = Infallible>`
//
//   This is the core Tower Service trait. It means:
//   * **Input**: Takes an HTTP request with a streaming body (`Incoming` from Hyper)
//   * **Output**: Returns an HTTP response with body type `B`
//   * **Error**: Must be `Infallible`, meaning the service never returns errors at the
//     Tower level. Any application errors must be converted into HTTP responses
//     (e.g., 500 Internal Server Error) before reaching this layer.
//
// - `S: Clone + Send + 'static`
//   * **Clone**: Each HTTP/1.1 or HTTP/2 connection may handle multiple requests
//     sequentially or concurrently. The service must be cloneable so each request
//     can get its own copy.
//   * **Send**: The service will be moved into a spawned Tokio task, so it must be
//     safe to send across thread boundaries.
//   * **'static**: No borrowed references - the service must own all its data since
//     it will outlive the connection setup phase.
//
// - `S::Future: Send`
//   The future returned by `Service::call()` must also be `Send` so it can be
//   polled from any thread in Tokio's thread pool.
//
// ## MakeService Bounds (M)
//
// The `M` type parameter represents a **service factory** - a Tower service that
// creates a new `S` service for each incoming connection. This allows customizing
// services based on connection metadata (remote address, TLS info, etc.).
//
// Connection Info → Service Factory → Per-Connection Service
//
// Required bounds:
// - `M: for<'a> Service<IncomingStream<'a, L>, Error = Infallible, Response = S>`
//
//   This is the service factory itself:
//   * **Input**: `IncomingStream<'a, L>` - A struct containing connection metadata:
//     - `io: &'a TokioIo<L::Io>` - A borrowed reference to the connection's IO stream
//     - `remote_addr: L::Addr` - The remote address of the client
//
//   * **Output**: Returns a new `S` service instance for this specific connection
//
//   * **Error**: Must be `Infallible` - service creation must never fail
//
//   * **Higher-Rank Trait Bound (`for<'a>`)**: The factory must work
//     with `IncomingStream` that borrows the IO with *any* lifetime `'a`. This is
//     necessary because the IO is borrowed only temporarily during service creation,
//     and we don't know the specific lifetime at compile time.
//
// - `for<'a> <M as Service<IncomingStream<'a, L>>>::Future: Send`
//
//   The future returned by calling the make_service must be `Send` for any lifetime,
//   so it can be awaited across threads while creating the service.
//
// ## Example Flow
//
// ```text
// 1. Listener.accept() → (io, remote_addr)
// 2. make_service.call(IncomingStream { io: &io, remote_addr }) → Future<Output = S>
// 3. service.call(request) → Future<Output = Response>
// 4. Repeat step 3 for each request on the connection
// ```
//
// ## Why These Bounds Matter
//
// 1. **Services can be spawned onto Tokio tasks** (Send + 'static)
// 2. **Multiple requests can be handled per connection** (Clone)
// 3. **Error handling is infallible** - errors become HTTP responses, not Tower errors
// 4. **The MakeService works with borrowed connection info** - via HRTB with IncomingStream
//    This allows inspection of connection metadata without transferring ownership
//
// ============================================================================

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
/// - The underlying IO type (for connection configuration)
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
/// `make_service.call()` invocation. After the service is created, the IO is
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
    /// Reference to the IO for this connection
    pub io: &'a TokioIo<L::Io>,
    /// Remote address of the client
    pub remote_addr: L::Addr,
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
/// By default, this limits concurrently accepted connections to 16,384. Use
/// [`Serve::max_connections`] to configure the limit or
/// [`Serve::disable_connection_limit`] to disable it.
///
/// A 30-second deadline bounds the first request head for either HTTP version and
/// subsequent HTTP/1 header reads. Per-request header-read timeout protection is
/// currently HTTP/1-only; later HTTP/2 streams have no header-read deadline. The
/// default connection builder also applies a 20-second HTTP/2 keep-alive ping interval,
/// which does not enforce a header-read deadline.
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
/// let listener = TcpListener::bind("127.0.0.1:3000").await.unwrap();
/// aws_smithy_http_server::serve(listener, app.into_make_service()).await.unwrap();
/// ```
///
/// Serving with middleware applied:
///
/// ```rust,ignore
/// use tokio::net::TcpListener;
/// use tower::Layer;
/// use tower_http::timeout::TimeoutLayer;
/// use http::StatusCode;
/// use std::time::Duration;
/// use aws_smithy_http_server::routing::IntoMakeService;
///
/// let app = /* ... build service ... */;
/// let app = TimeoutLayer::new(Duration::from_secs(30)).layer(app);
///
/// let listener = TcpListener::bind("127.0.0.1:3000").await.unwrap();
/// aws_smithy_http_server::serve(listener, IntoMakeService::new(app)).await.unwrap();
/// ```
///
/// For graceful shutdown:
///
/// ```rust,ignore
/// use tokio::net::TcpListener;
/// use tokio::signal;
///
/// let listener = TcpListener::bind("127.0.0.1:3000").await.unwrap();
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
/// let listener = TcpListener::bind("127.0.0.1:3000").await.unwrap();
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
    // Body bounds: see module documentation for details
    B: HttpBody + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn StdError + Send + Sync>>,
    // Service bounds: see module documentation for details
    S: Service<http::Request<Incoming>, Response = http::Response<B>, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
    // MakeService bounds: see module documentation for details
    M: for<'a> Service<IncomingStream<'a, L>, Error = Infallible, Response = S>,
{
    Serve {
        listener,
        make_service,
        settings: ServeSettings::default(),
        _marker: PhantomData,
    }
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
/// - [`configure_hyper`](Self::configure_hyper) - Configure Hyper's automatic connection builder
/// - [`max_connections`](Self::max_connections) - Configure the accepted-connection limit
/// - [`header_read_timeout`](Self::header_read_timeout) - Configure the request-header deadline
/// - [`with_graceful_shutdown`](Self::with_graceful_shutdown) - Enable graceful shutdown
/// - [`local_addr`](Self::local_addr) - Get the bound address
///
/// Created by [`serve`].
#[must_use = "Serve does nothing until you `.await` or call `.into_future()` on it"]
pub struct Serve<L, M, S, B> {
    listener: L,
    make_service: M,
    settings: ServeSettings,
    _marker: PhantomData<(S, B)>,
}

impl<L, M, S, B> fmt::Debug for Serve<L, M, S, B>
where
    L: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Serve")
            .field("listener", &self.listener)
            .field("has_hyper_config", &self.settings.hyper_builder.is_some())
            .field("max_connections", &self.settings.max_connections)
            .field("header_read_timeout", &self.settings.header_read_timeout)
            .finish_non_exhaustive()
    }
}

impl<L, M, S, B> Serve<L, M, S, B>
where
    L: Listener,
{
    /// Configure the underlying Hyper connection builder.
    ///
    /// This allows you to customize Hyper's HTTP/1 and HTTP/2 settings,
    /// such as timeouts, max concurrent streams, keep-alive behavior, etc.
    ///
    /// **Note:** Calling this replaces the default builder entirely, opting out of the built-in
    /// HTTP/2 defaults. You are responsible for setting your own HTTP/2 timeouts.
    /// The connection limit and [`Self::header_read_timeout`] remain enabled.
    /// When HTTP/1 is enabled, `serve` supplies its timer and applies
    /// `header_read_timeout` after this configuration, overriding any HTTP/1
    /// header-read timeout set on the builder.
    ///
    /// `http1_only()` and `http2_only()` restrict the accepted protocol and use
    /// Hyper's regular connection API without HTTP/1 upgrades. When both
    /// protocols are enabled, `serve` uses `serve_connection_with_upgrades`.
    /// For HTTP/1-only upgrades, use a custom serve loop.
    ///
    /// The configuration callback runs once when this method is called. The
    /// configured builder is shared across connections.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use std::time::Duration;
    ///
    /// serve(listener, service)
    ///     .configure_hyper(|mut builder| {
    ///         builder.http1().keep_alive(true);
    ///         builder.http2().max_concurrent_streams(200);
    ///         builder
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
        self.settings.hyper_builder = Some(Arc::new(f(builder)));
        self
    }

    /// Set the maximum number of concurrently accepted connections.
    ///
    /// The default limit is 16,384. Once the limit is reached, `serve` stops
    /// accepting connections until an existing connection closes.
    ///
    /// Passing `0` disables the limit, equivalent to
    /// [`disable_connection_limit`](Self::disable_connection_limit). Values
    /// above [`tokio::sync::Semaphore::MAX_PERMITS`] are clamped to that value.
    pub fn max_connections(mut self, max: usize) -> Self {
        self.settings.max_connections = (max > 0).then_some(max);
        self
    }

    /// Disable the default accepted-connection limit.
    pub fn disable_connection_limit(mut self) -> Self {
        self.settings.max_connections = None;
        self
    }

    /// Set the timeout for receiving a complete request head.
    ///
    /// The default is 30 seconds. The first-request deadline starts when the
    /// connection task begins driving Hyper, after `make_service` finishes. It
    /// includes protocol negotiation and header parsing and does not reset as
    /// bytes arrive. It ends before Tower readiness, middleware, or body reads.
    ///
    /// The same setting configures Hyper's HTTP/1 header-read timeout for later
    /// requests. **Per-request header-read timeout protection is currently supported
    /// only for HTTP/1.** HTTP/2 is covered only until the first request head arrives;
    /// later HTTP/2 streams have no header-read deadline. This deadline does not
    /// limit body reads or time spent waiting for server events.
    /// This setting takes precedence over the HTTP/1 header-read timeout supplied
    /// through [`Self::configure_hyper`]. Pass `None` or `Some(Duration::ZERO)` to
    /// disable both deadlines.
    pub fn header_read_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.settings.header_read_timeout = timeout.filter(|timeout| !timeout.is_zero());
        self
    }

    /// Enable graceful shutdown for the server.
    pub fn with_graceful_shutdown<F>(self, signal: F) -> ServeWithGracefulShutdown<L, M, S, F, B>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        ServeWithGracefulShutdown { serve: self, signal }
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
    // Body bounds
    B: HttpBody + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn StdError + Send + Sync>>,
    // Service bounds
    S: Service<http::Request<Incoming>, Response = http::Response<B>, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
    // MakeService bounds
    M: for<'a> Service<IncomingStream<'a, L>, Error = Infallible, Response = S> + Send + 'static,
    for<'a> <M as Service<IncomingStream<'a, L>>>::Future: Send,
{
    type Output = io::Result<()>;
    type IntoFuture = Pin<Box<dyn Future<Output = io::Result<()>> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let Self {
                mut listener,
                mut make_service,
                settings:
                    ServeSettings {
                        hyper_builder,
                        max_connections,
                        header_read_timeout,
                        ..
                    },
                _marker,
            } = self;
            let connection_limiter = max_connections.map(ConnectionLimit::new).map(Arc::new);
            let hyper_builder = connection_builder(hyper_builder, header_read_timeout);

            loop {
                let connection_permit = acquire_connection_permit(connection_limiter.as_ref()).await;
                let (io, remote_addr) = listener.accept().await;
                handle_connection::<L, M, S, B>(
                    &mut make_service,
                    io,
                    remote_addr,
                    &hyper_builder,
                    header_read_timeout,
                    None,
                    connection_permit,
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
    serve: Serve<L, M, S, B>,
    signal: F,
}

impl<L, M, S, F, B> fmt::Debug for ServeWithGracefulShutdown<L, M, S, F, B>
where
    L: Listener + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServeWithGracefulShutdown")
            .field("listener", &self.serve.listener)
            .field("has_hyper_config", &self.serve.settings.hyper_builder.is_some())
            .field("shutdown_timeout", &self.serve.settings.shutdown_timeout)
            .field("max_connections", &self.serve.settings.max_connections)
            .field("header_read_timeout", &self.serve.settings.header_read_timeout)
            .finish_non_exhaustive()
    }
}

impl<L: Listener, M, S, F, B> ServeWithGracefulShutdown<L, M, S, F, B> {
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
        self.serve.settings.shutdown_timeout = Some(timeout);
        self
    }

    /// Set the maximum number of concurrently accepted connections.
    ///
    /// The default limit is 16,384. Once the limit is reached, `serve` stops
    /// accepting connections until an existing connection closes.
    ///
    /// Passing `0` disables the limit, equivalent to
    /// [`disable_connection_limit`](Self::disable_connection_limit). Values
    /// above [`tokio::sync::Semaphore::MAX_PERMITS`] are clamped to that value.
    pub fn max_connections(mut self, max: usize) -> Self {
        self.serve = self.serve.max_connections(max);
        self
    }

    /// Disable the default accepted-connection limit.
    pub fn disable_connection_limit(mut self) -> Self {
        self.serve = self.serve.disable_connection_limit();
        self
    }

    /// Set the timeout for receiving a complete request head.
    ///
    /// The default is 30 seconds. Pass `None` or `Some(Duration::ZERO)` to disable it.
    /// See [`Serve::header_read_timeout`] for details.
    pub fn header_read_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.serve = self.serve.header_read_timeout(timeout);
        self
    }

    /// Returns the local address this server is bound to.
    pub fn local_addr(&self) -> io::Result<L::Addr> {
        self.serve.local_addr()
    }
}

// Implement IntoFuture so we can await WithGracefulShutdown directly
impl<L, M, S, F, B> IntoFuture for ServeWithGracefulShutdown<L, M, S, F, B>
where
    L: Listener,
    L::Addr: Debug,
    // Body bounds
    B: HttpBody + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn StdError + Send + Sync>>,
    // Service bounds
    S: Service<http::Request<Incoming>, Response = http::Response<B>, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
    // MakeService bounds
    M: for<'a> Service<IncomingStream<'a, L>, Error = Infallible, Response = S> + Send + 'static,
    for<'a> <M as Service<IncomingStream<'a, L>>>::Future: Send,
    // Shutdown signal
    F: Future<Output = ()> + Send + 'static,
{
    type Output = io::Result<()>;
    type IntoFuture = Pin<Box<dyn Future<Output = io::Result<()>> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let Self {
                serve:
                    Serve {
                        mut listener,
                        mut make_service,
                        settings:
                            ServeSettings {
                                hyper_builder,
                                max_connections,
                                header_read_timeout,
                                shutdown_timeout,
                            },
                        _marker,
                    },
                signal,
            } = self;
            let connection_limiter = max_connections.map(ConnectionLimit::new).map(Arc::new);
            let hyper_builder = connection_builder(hyper_builder, header_read_timeout);

            // Initialize graceful shutdown
            let graceful = GracefulShutdown::new();
            let mut signal = std::pin::pin!(signal);

            loop {
                let connection_permit = tokio::select! {
                    permit = acquire_connection_permit(connection_limiter.as_ref()) => permit,
                    _ = signal.as_mut() => {
                        tracing::trace!("received graceful shutdown signal, not accepting new connections");
                        break;
                    }
                };

                tokio::select! {
                    (io, remote_addr) = listener.accept() => {
                        handle_connection::<L, M, S, B>(
                            &mut make_service,
                            io,
                            remote_addr,
                            &hyper_builder,
                            header_read_timeout,
                            Some(&graceful),
                            connection_permit,
                        )
                        .await;
                    }
                    _ = signal.as_mut() => {
                        tracing::trace!("received graceful shutdown signal, not accepting new connections");
                        break;
                    }
                }
            }

            drop(listener);

            tracing::trace!("waiting for in-flight connections to finish");

            // Wait for all in-flight connections (with optional timeout)
            match shutdown_timeout {
                Some(timeout) => match tokio::time::timeout(timeout, graceful.shutdown()).await {
                    Ok(_) => {
                        tracing::trace!("all in-flight connections completed during graceful shutdown");
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
                    tracing::trace!("all in-flight connections completed during graceful shutdown");
                }
            }

            Ok(())
        })
    }
}

/// Connection handling function.
///
/// Handles connections with HTTP upgrade support and optional graceful shutdown.
async fn handle_connection<L, M, S, B>(
    make_service: &mut M,
    conn_io: <L as Listener>::Io,
    remote_addr: <L as Listener>::Addr,
    hyper_builder: &Arc<Builder<TokioExecutor>>,
    header_read_timeout: Option<Duration>,
    graceful: Option<&GracefulShutdown>,
    connection_permit: Option<OwnedSemaphorePermit>,
) where
    L: Listener,
    L::Addr: Debug,
    // Body bounds
    B: HttpBody + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn StdError + Send + Sync>>,
    // Service bounds
    S: Service<http::Request<Incoming>, Response = http::Response<B>, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send,
    // MakeService bounds
    M: for<'a> Service<IncomingStream<'a, L>, Error = Infallible, Response = S> + Send + 'static,
    for<'a> <M as Service<IncomingStream<'a, L>>>::Future: Send,
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

    let builder = Arc::clone(hyper_builder);

    // Preserve IncomingStream's original IO type while constructing the service.
    // Hyper then owns the permit through the IO, including after an HTTP upgrade.
    let tokio_io = TokioIo::new(ConnLimiterIo::new(tokio_io.into_inner(), connection_permit));
    tokio::spawn(async move {
        let result = if let Some(timeout) = header_read_timeout {
            let (service, deadline) = HeaderReadTimeoutService::new(hyper_service, timeout);
            serve_connection(&builder, tokio_io, service, watcher, Some(deadline)).await
        } else {
            serve_connection(&builder, tokio_io, hyper_service, watcher, None).await
        };

        if let Err(err) = result {
            tracing::trace!(error = ?err, "failed to serve connection");
        }
    });
}

async fn serve_connection<I, S, B>(
    builder: &Builder<TokioExecutor>,
    io: I,
    service: S,
    watcher: Option<Watcher>,
    header_deadline: Option<HeaderReadDeadline>,
) -> io::Result<()>
where
    I: hyper::rt::Read + hyper::rt::Write + Unpin + Send + 'static,
    S: hyper::service::Service<http::Request<Incoming>, Response = http::Response<B>> + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Into<Box<dyn StdError + Send + Sync>>,
    B: HttpBody + Send + 'static,
    B::Data: Send,
    B::Error: Into<Box<dyn StdError + Send + Sync>>,
{
    // Hyper's upgrade-capable API ignores http1_only()/http2_only(), so use
    // it only when both protocols are enabled.
    if builder.is_http1_available() && builder.is_http2_available() {
        let connection = builder.serve_connection_with_upgrades(io, service);
        drive_connection(connection, watcher, header_deadline).await
    } else {
        let connection = builder.serve_connection(io, service);
        drive_connection(connection, watcher, header_deadline).await
    }
}

async fn drive_connection<C>(
    connection: C,
    watcher: Option<Watcher>,
    header_deadline: Option<HeaderReadDeadline>,
) -> io::Result<()>
where
    C: GracefulConnection,
    C::Error: Into<Box<dyn StdError + Send + Sync>>,
{
    if let Some(watcher) = watcher {
        let connection = watcher.watch(connection);
        if let Some(deadline) = header_deadline {
            deadline.run(connection).await
        } else {
            connection.await.map_err(io::Error::other)
        }
    } else if let Some(deadline) = header_deadline {
        deadline.run(connection).await
    } else {
        connection.await.map_err(io::Error::other)
    }
}

fn connection_builder(
    configured: Option<Arc<Builder<TokioExecutor>>>,
    header_read_timeout: Option<Duration>,
) -> Arc<Builder<TokioExecutor>> {
    let mut builder = configured
        .map(Arc::unwrap_or_clone)
        .unwrap_or_else(default_connection_builder);
    if builder.is_http1_available() {
        builder
            .http1()
            .timer(TokioTimer::new())
            .header_read_timeout(header_read_timeout);
    }
    Arc::new(builder)
}

async fn acquire_connection_permit(connection_limiter: Option<&Arc<ConnectionLimit>>) -> Option<OwnedSemaphorePermit> {
    match connection_limiter {
        Some(limit) => Some(limit.acquire().await),
        None => None,
    }
}
