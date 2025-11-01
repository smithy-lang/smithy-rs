/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Connection strategy traits for handling HTTP/1 and HTTP/2 connections.
//!
//! This module provides a trait-based approach for selecting connection handling
//! strategies at compile time, avoiding runtime branching.

use std::fmt::Debug;
use std::future::Future;

use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use hyper_util::server::graceful::{GracefulConnection, Watcher};
use hyper_util::service::TowerToHyperService;

/// Strategy trait for handling HTTP connections.
///
/// This trait allows compile-time selection of connection handling methods,
/// avoiding runtime if statements by using monomorphization.
pub trait ConnectionStrategy {
    /// Serve a connection using this strategy.
    fn serve<S, B, I>(
        builder: &mut Builder<TokioExecutor>,
        io: TokioIo<I>,
        service: TowerToHyperService<S>,
    ) -> impl GracefulConnection<Error: Debug> + Send
    where
        I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
        S: tower::Service<http::Request<hyper::body::Incoming>, Response = http::Response<B>>
            + Clone
            + Send
            + 'static,
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
        builder: &mut Builder<TokioExecutor>,
        io: TokioIo<I>,
        service: TowerToHyperService<S>,
    ) -> impl GracefulConnection<Error: Debug> + Send
    where
        I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
        S: tower::Service<http::Request<hyper::body::Incoming>, Response = http::Response<B>>
            + Clone
            + Send
            + 'static,
        S::Future: Send,
        S::Error: std::error::Error + Send + Sync + 'static,
        B: http_body::Body + Send + 'static,
        B::Data: Send,
        B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
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
        builder: &mut Builder<TokioExecutor>,
        io: TokioIo<I>,
        service: TowerToHyperService<S>,
    ) -> impl GracefulConnection<Error: Debug> + Send
    where
        I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
        S: tower::Service<http::Request<hyper::body::Incoming>, Response = http::Response<B>>
            + Clone
            + Send
            + 'static,
        S::Future: Send,
        S::Error: std::error::Error + Send + Sync + 'static,
        B: http_body::Body + Send + 'static,
        B::Data: Send,
        B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        builder.serve_connection(io, service)
    }
}

/// Strategy trait for handling graceful shutdown of connections.
///
/// This trait allows compile-time selection of shutdown handling,
/// avoiding runtime branching on whether graceful shutdown is enabled.
pub trait ShutdownStrategy {
    /// Execute the connection future with appropriate shutdown handling.
    ///
    /// For strategies that use graceful shutdown, a `Watcher` must be provided.
    ///
    /// The default implementation simply returns the connection without
    /// any graceful shutdown coordination.
    fn execute<C>(
        _watcher: Option<Watcher>,
        conn: C,
    ) -> impl Future<Output = C::Output> + Send
    where
        C: GracefulConnection + Send,
    {
        conn
    }
}

/// Shutdown strategy that uses graceful shutdown coordination.
///
/// This strategy wraps the connection with a watcher that will trigger
/// graceful shutdown when the shutdown signal is received.
#[derive(Debug, Clone, Copy, Default)]
pub struct WithGracefulShutdown;

impl ShutdownStrategy for WithGracefulShutdown {
    fn execute<C>(
        watcher: Option<Watcher>,
        conn: C,
    ) -> impl Future<Output = C::Output> + Send
    where
        C: GracefulConnection + Send,
    {
        watcher.expect("WithGracefulShutdown requires a watcher").watch(conn)
    }
}

/// Shutdown strategy that does not use graceful shutdown.
///
/// This strategy simply awaits the connection future directly without
/// any shutdown coordination. It uses the default trait implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct WithoutGracefulShutdown;

impl ShutdownStrategy for WithoutGracefulShutdown {
    // Uses default implementation
}
