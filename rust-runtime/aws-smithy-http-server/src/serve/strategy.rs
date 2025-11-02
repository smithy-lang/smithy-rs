/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Connection and shutdown strategy traits for handling HTTP/1 and HTTP/2 connections.
//!
//! This module provides trait-based approaches for selecting connection handling
//! and shutdown strategies at compile time, avoiding runtime branching.

use std::fmt::Debug;
use std::future::Future;

use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use hyper_util::server::graceful::{GracefulConnection, GracefulShutdown, Watcher};
use hyper_util::service::TowerToHyperService;

/// Strategy trait for handling HTTP connections.
///
/// This trait allows compile-time selection of connection handling methods,
/// avoiding runtime if statements by using monomorphization.
pub trait ConnectionStrategy {
    /// Serve a connection using this strategy.
    ///
    /// Returns a connection future that implements `GracefulConnection` - this is
    /// hyper's interface for connections that can be gracefully shut down.
    ///
    /// Note: This only requires an immutable reference to the builder because
    /// Hyper's `serve_connection` methods don't mutate the builder.
    fn serve<S, B, I>(
        builder: &Builder<TokioExecutor>,
        io: TokioIo<I>,
        service: TowerToHyperService<S>,
    ) -> impl GracefulConnection<Error: Debug> + Send
    where
        I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
        S: tower::Service<http::Request<hyper::body::Incoming>, Response = http::Response<B>> + Clone + Send + 'static,
        S::Future: Send,
        S::Error: std::error::Error + Send + Sync + 'static,
        B: http_body::Body + Send + 'static,
        B::Data: Send,
        B::Error: Into<Box<dyn std::error::Error + Send + Sync>>;
}

/// Connection strategy that supports HTTP/1 upgrades.
///
/// This strategy uses `serve_connection_with_upgrades` to enable HTTP/1.1 upgrade
/// mechanisms, allowing protocol negotiation (e.g., WebSocket, HTTP/2 via h2c).
#[derive(Debug, Clone, Copy, Default)]
pub struct WithUpgrades;

impl ConnectionStrategy for WithUpgrades {
    fn serve<S, B, I>(
        builder: &Builder<TokioExecutor>,
        io: TokioIo<I>,
        service: TowerToHyperService<S>,
    ) -> impl GracefulConnection<Error: Debug> + Send
    where
        I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
        // Body bounds
        B: http_body::Body + Send + 'static,
        B::Data: Send,
        B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
        // Service Bounds
        S: tower::Service<http::Request<hyper::body::Incoming>, Response = http::Response<B>> + Clone + Send + 'static,
        S::Future: Send,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        builder.serve_connection_with_upgrades(io, service)
    }
}

/// Connection strategy that does not support upgrades.
///
/// This strategy uses `serve_connection` for standard HTTP/1 or HTTP/2 connections
/// without upgrade support. This is more efficient when upgrades are not needed,
/// as it skips preface reading.
#[derive(Debug, Clone, Copy, Default)]
pub struct WithoutUpgrades;

impl ConnectionStrategy for WithoutUpgrades {
    fn serve<S, B, I>(
        builder: &Builder<TokioExecutor>,
        io: TokioIo<I>,
        service: TowerToHyperService<S>,
    ) -> impl GracefulConnection<Error: Debug> + Send
    where
        I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
        // Body Bounds
        B: http_body::Body + Send + 'static,
        B::Data: Send,
        B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
        // Service Bounds
        S: tower::Service<http::Request<hyper::body::Incoming>, Response = http::Response<B>> + Clone + Send + 'static,
        S::Future: Send,
        S::Error: std::error::Error + Send + Sync + 'static,
    {
        builder.serve_connection(io, service)
    }
}

/// Strategy trait for managing graceful shutdown lifecycle.
///
/// This trait encapsulates the complete lifecycle of graceful shutdown coordination:
/// - Initialization: Creating the shutdown coordinator
/// - Per-connection tracking: Creating handles for each connection
/// - Execution: Running connections with shutdown awareness
/// - Finalization: Waiting for all connections to complete
///
/// By managing the full lifecycle, this trait enables swapping shutdown mechanisms
/// without changing the accept loop. For example, you could implement an alternative
/// to hyper_util's GracefulShutdown (like Axum's watch-channel approach) by providing
/// a new implementation of this trait.
///
/// # Design Philosophy
///
/// The trait separates the **coordinator** (shared state tracking all connections)
/// from the **handle** (per-connection tracking token). This allows:
///
/// 1. **Encapsulation**: All shutdown logic lives in one place
/// 2. **Flexibility**: Different implementations can use different coordination mechanisms
/// 3. **Zero-cost abstraction**: When not using shutdown, no overhead is incurred
/// 4. **Separation of concerns**: Only lifecycle methods are in the trait, not observability
///
/// # Observability and Metrics
///
/// This trait intentionally does NOT include observability methods (like counting active
/// connections). Metrics and observability are orthogonal concerns that should be handled
/// separately:
///
/// - If you need metrics, access the `Coordinator` type directly
/// - For example: `coordinator.count()` works on `GracefulShutdown`
/// - Or implement a separate `ShutdownMetrics` trait for your use case
///
/// This keeps the trait minimal and easy to implement, while allowing rich observability
/// for implementations that support it.
///
/// # Why GracefulConnection Dependency?
///
/// The `execute` method requires connections to implement `GracefulConnection` (from hyper_util).
/// This is intentional:
///
/// - We're using hyper for HTTP serving, and hyper's connections implement `GracefulConnection`
/// - This allows shutdown strategies to call `graceful_shutdown()` when needed
/// - Strategies that don't need graceful shutdown can simply ignore the method
/// - `GracefulConnection` is a reasonable abstraction: `Future + graceful_shutdown()`
///
/// While this creates a dependency on hyper_util's trait, it's a natural consequence of
/// using hyper, and it provides a clean interface for shutdown coordination
///
/// # Examples
///
/// The default implementation uses `hyper_util::server::graceful::GracefulShutdown`:
///
/// ```rust,ignore
/// // In the accept loop:
/// let coordinator = HyperUtilShutdown::init();
///
/// loop {
///     let (io, addr) = listener.accept().await;
///     let handle = HyperUtilShutdown::track_connection(&coordinator);
///     let conn = builder.serve_connection(io, service);
///
///     tokio::spawn(async move {
///         HyperUtilShutdown::execute(handle, conn).await
///     });
/// }
///
/// // After shutdown signal:
/// HyperUtilShutdown::finalize(coordinator).await;
/// ```
pub trait ShutdownStrategy: Send + 'static {
    /// The coordinator type that manages shutdown state.
    ///
    /// This is the shared state that tracks all active connections. For example:
    /// - `hyper_util::GracefulShutdown` uses a watch channel sender
    /// - An Axum-style implementation might use two watch channels
    /// - A no-op implementation would use `()`
    type Coordinator: Send + 'static;

    /// Per-connection handle for tracking.
    ///
    /// This is created for each connection and used to track its lifecycle.
    /// When dropped, it should signal that the connection has completed. For example:
    /// - `hyper_util::Watcher` holds a watch channel receiver
    /// - An Axum-style implementation might hold both signal and close receivers
    /// - A no-op implementation would use `()`
    type Handle: Send + 'static;

    /// Initialize the shutdown coordinator.
    ///
    /// Called once at server startup to create the shared shutdown state.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let coordinator = MyShutdown::init();
    /// // coordinator is now ready to track connections
    /// ```
    fn init() -> Self::Coordinator;

    /// Create a handle for tracking a new connection.
    ///
    /// Called once per accepted connection to create a tracking token.
    /// The handle should maintain a reference to the coordinator state
    /// so that when it's dropped, the coordinator knows the connection ended.
    ///
    /// # Arguments
    ///
    /// * `coordinator` - Reference to the shared shutdown coordinator
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let handle = MyShutdown::track_connection(&coordinator);
    /// // handle is now associated with this connection
    /// ```
    fn track_connection(coordinator: &Self::Coordinator) -> Self::Handle;

    /// Execute the connection with shutdown awareness.
    ///
    /// This is where the shutdown mechanism integrates with the connection future.
    /// The implementation can interact with the connection in whatever way it needs:
    ///
    /// - Call `conn.graceful_shutdown()` if the connection implements that
    /// - Race with shutdown signals using `tokio::select!`
    /// - Simply poll the connection directly (no-op shutdown)
    /// - Use any other shutdown mechanism
    ///
    /// # Arguments
    ///
    /// * `handle` - Per-connection tracking handle
    /// * `conn` - The connection future to execute
    ///
    /// # Returns
    ///
    /// The same output as the connection future would produce.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let result = MyShutdown::execute(handle, conn).await;
    /// // Connection has completed (normally or via shutdown)
    /// ```
    ///
    /// # Why GracefulConnection?
    ///
    /// The connection must implement `GracefulConnection` (from hyper_util), which is
    /// a `Future` with an additional `graceful_shutdown()` method. This bound is
    /// required because:
    ///
    /// 1. **Hyper's connections implement this** - All connections from
    ///    `ConnectionStrategy::serve()` already implement `GracefulConnection`
    ///
    /// 2. **Enables graceful shutdown** - Strategies like `HyperUtilShutdown` need
    ///    to call `graceful_shutdown()` to properly drain in-flight requests
    ///
    /// 3. **Doesn't force usage** - Strategies that don't need graceful shutdown
    ///    (like `NoShutdown`) can simply ignore the method and just poll the future
    ///
    /// 4. **Clean abstraction** - `GracefulConnection` is a reasonable interface for
    ///    "a connection that can be shut down gracefully" - it's not overly specific
    ///    to hyper_util's implementation
    ///
    /// While this creates a dependency on hyper_util's trait, it's a natural consequence
    /// of using hyper for HTTP serving. Alternative approaches (like defining our own
    /// trait) would just recreate the same interface.
    fn execute<C>(handle: Self::Handle, conn: C) -> impl Future<Output = C::Output> + Send
    where
        C: GracefulConnection + Send;

    /// Wait for all tracked connections to complete.
    ///
    /// Called after the shutdown signal is received and no new connections
    /// are being accepted. This should:
    ///
    /// 1. Signal all tracked connections to shut down gracefully
    /// 2. Wait until all connection handles have been dropped
    /// 3. Return once all connections are complete
    ///
    /// # Arguments
    ///
    /// * `coordinator` - The shutdown coordinator (consumed)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Signal received, stop accepting connections
    /// drop(listener);
    ///
    /// // Wait for all in-flight connections
    /// MyShutdown::finalize(coordinator).await;
    /// ```
    fn finalize(coordinator: Self::Coordinator) -> impl Future<Output = ()> + Send;
}

/// Shutdown strategy using `hyper_util::server::graceful::GracefulShutdown`.
///
/// This is the default and recommended shutdown strategy. It uses hyper-util's
/// built-in graceful shutdown coordinator based on watch channels.
///
/// # How It Works
///
/// 1. **Initialization**: Creates a `GracefulShutdown` with a watch channel
/// 2. **Per-connection tracking**: Each connection gets a `Watcher` (receiver clone)
/// 3. **Execution**: Uses `Watcher::watch()` to wrap connections
/// 4. **Finalization**: Calls `GracefulShutdown::shutdown()` to signal and wait
///
/// When `finalize` is called, it:
/// - Sends a signal through the watch channel
/// - Waits for all Watcher receivers to be dropped
/// - Returns once all connections have completed
///
/// # Example
///
/// ```rust,ignore
/// use aws_smithy_http_server::serve::strategy::HyperUtilShutdown;
///
/// // This is the default - you usually don't need to specify it explicitly
/// serve(listener, app.into_make_service())
///     .with_graceful_shutdown(shutdown_signal())
///     .await?;
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct HyperUtilShutdown;

impl ShutdownStrategy for HyperUtilShutdown {
    type Coordinator = GracefulShutdown;
    type Handle = Watcher;

    fn init() -> Self::Coordinator {
        GracefulShutdown::new()
    }

    fn track_connection(coordinator: &Self::Coordinator) -> Self::Handle {
        coordinator.watcher()
    }

    fn execute<C>(handle: Self::Handle, conn: C) -> impl Future<Output = C::Output> + Send
    where
        C: GracefulConnection + Send,
    {
        // Watcher::watch wraps the connection and calls graceful_shutdown() when signaled
        handle.watch(conn)
    }

    fn finalize(coordinator: Self::Coordinator) -> impl Future<Output = ()> + Send {
        coordinator.shutdown()
    }
}

/// Shutdown strategy that does no shutdown coordination.
///
/// This is used when graceful shutdown is not enabled. It has zero runtime overhead:
/// - No watch channels are allocated
/// - No select polling overhead
/// - Connections run directly without wrappers
///
/// # How It Works
///
/// - **Coordinator**: Unit type `()` - no state
/// - **Handle**: Unit type `()` - no tracking
/// - **Execute**: Directly awaits the connection future
/// - **Finalize**: Does nothing (instant return)
///
/// # Example
///
/// ```rust,ignore
/// // When you don't call .with_graceful_shutdown(), this is used:
/// serve(listener, app.into_make_service()).await?;
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct NoShutdown;

impl ShutdownStrategy for NoShutdown {
    type Coordinator = ();
    type Handle = ();

    fn init() -> Self::Coordinator {
        ()
    }

    fn track_connection(_coordinator: &Self::Coordinator) -> Self::Handle {
        ()
    }

    fn execute<C>(_handle: Self::Handle, conn: C) -> impl Future<Output = C::Output> + Send
    where
        C: GracefulConnection + Send,
    {
        // We don't call graceful_shutdown() - just poll the connection directly.
        // This still works because GracefulConnection is a Future, and we're
        // just using it as a Future. The graceful_shutdown() method is ignored.
        conn
    }

    fn finalize(_coordinator: Self::Coordinator) -> impl Future<Output = ()> + Send {
        async {}
    }
}

