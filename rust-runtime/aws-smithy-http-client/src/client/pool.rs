/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Connection pool for the v2 HTTP client.
//!
//! Manages the lifecycle of HTTP connections including:
//! - Connection state tracking (idle duration, remote address, poisoned flag)
//! - Health checks on checkout
//! - Connection poisoning
//! - Idle connection eviction

mod connection;
mod handshake;
mod vendored_cache;

/// Connection-caching pool layer.
///
/// Currently re-exports [`vendored_cache`], a copy of hyper-util's
/// `pool::cache` module with SDK-specific modifications. The re-export
/// insulates the rest of the pool from the vendoring detail: call sites
/// use `cache::...` regardless of whether the implementation is vendored
/// or, later, fully owned.
mod cache {
    pub(crate) use super::vendored_cache::*;
}

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use aws_smithy_types::body::SdkBody;
use hyper_util::client::legacy::connect::Connection as HyperConnection;
use hyper_util::client::pool as hpool;
use hyper_util::rt::TokioExecutor;
use tower::{Service, ServiceExt};

use connection::ManagedConnection;
use handshake::{H1ConnectAndHandshake, H2ConnectAndHandshake};

pub(crate) type BoxError = Box<dyn std::error::Error + Send + Sync>;
type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Key for per-host connection pool routing.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PoolKey {
    scheme: http_1x::uri::Scheme,
    authority: http_1x::uri::Authority,
}

impl PoolKey {
    fn from_uri(uri: &http_1x::Uri) -> Option<Self> {
        Some(Self {
            scheme: uri.scheme()?.clone(),
            authority: uri.authority()?.clone(),
        })
    }
}

/// Build a connection pool for the given connector.
pub(crate) fn build_pool<C, IO>(connector: C) -> ConnectionPool
where
    C: Service<http_1x::Uri, Response = IO> + Clone + Send + Sync + 'static,
    C::Error: Into<BoxError> + 'static,
    C::Future: Unpin + Send + 'static,
    IO: hyper::rt::Read + hyper::rt::Write + HyperConnection + Unpin + Send + 'static,
{
    let make_entry = move |_uri: &http_1x::Uri| -> Box<dyn PoolEntry> {
        let stack = hpool::negotiate::builder()
            .connect(connector.clone())
            .inspect(|conn: &IO| conn.connected().is_negotiated_h2())
            .fallback(tower::layer::layer_fn(|inspector| {
                cache::builder()
                    .executor(TokioExecutor::new())
                    .build(H1ConnectAndHandshake::new(inspector))
                    .map_response(CachedConnection::new)
            }))
            .upgrade(tower::layer::layer_fn(|inspected| {
                hpool::singleton::Singleton::new(H2ConnectAndHandshake::new(inspected))
            }))
            .build();
        Box::new(TypedPoolEntry(stack))
    };

    ConnectionPool {
        hosts: Mutex::new(HashMap::new()),
        make_entry: Box::new(make_entry),
    }
}

/// The connection pool.
///
/// Routes requests by (scheme, authority) to per-host pool stacks.
/// Each host gets a Negotiate stack that selects between HTTP/1.1 (Cache)
/// and HTTP/2 (Singleton) based on ALPN negotiation.
pub(crate) struct ConnectionPool {
    hosts: Mutex<HashMap<PoolKey, Box<dyn PoolEntry>>>,
    make_entry: Box<dyn Fn(&http_1x::Uri) -> Box<dyn PoolEntry> + Send + Sync>,
}

impl ConnectionPool {
    /// Send a request through the pool.
    ///
    /// Routes to the appropriate per-host pool stack, acquires a connection,
    /// and sends the request.
    pub(crate) async fn send_request(
        &self,
        uri: http_1x::Uri,
        req: http_1x::Request<SdkBody>,
    ) -> Result<http_1x::Response<hyper::body::Incoming>, BoxError> {
        let key = PoolKey::from_uri(&uri).ok_or("request URI must have scheme and authority")?;

        // Clone the per-host stack out of the lock. The stack is Clone
        // (cheap Arc clones internally). All I/O happens outside the lock.
        let fut = {
            let mut hosts = self.hosts.lock().unwrap();
            if !hosts.contains_key(&key) {
                hosts.insert(key.clone(), (self.make_entry)(&uri));
            }
            hosts.get_mut(&key).unwrap().acquire(uri)
        };

        let conn = fut.await?;
        conn.send(req).await
    }
}

/// Type-erased per-host pool entry.
///
/// Each host has one of these, wrapping the unnameable Negotiate stack.
trait PoolEntry: Send + Sync {
    /// Acquire a connection from this host's pool.
    fn acquire(&mut self, uri: http_1x::Uri) -> BoxFuture<Result<BoxedConnection, BoxError>>;
}

/// A type-erased connection that can send one request.
trait Connection: Send {
    fn send(
        self: Box<Self>,
        req: http_1x::Request<SdkBody>,
    ) -> BoxFuture<Result<http_1x::Response<hyper::body::Incoming>, BoxError>>;
}

type BoxedConnection = Box<dyn Connection>;

/// Concrete PoolEntry wrapping a Negotiate stack.
struct TypedPoolEntry<S>(S);

impl<S, Conn> PoolEntry for TypedPoolEntry<S>
where
    S: Service<http_1x::Uri, Response = Conn> + Clone + Send + Sync + 'static,
    S::Error: Into<BoxError> + 'static,
    S::Future: Send + 'static,
    Conn: Service<http_1x::Request<SdkBody>> + Send + 'static,
    Conn::Response: Into<http_1x::Response<hyper::body::Incoming>>,
    Conn::Error: Into<BoxError>,
    Conn::Future: Send + 'static,
{
    fn acquire(&mut self, uri: http_1x::Uri) -> BoxFuture<Result<BoxedConnection, BoxError>> {
        let mut svc = self.0.clone();
        Box::pin(async move {
            // Loop past stale idle entries (poisoned or dead). Converges because:
            //   - Cache idle set is bounded; each post-checkout `poll_ready` Err
            //     flips `is_closed` so `Cached::Drop` skips reinsertion, shrinking
            //     the set by one.
            //   - Singleton clears to `Empty` on `poll_ready` Err, forcing the
            //     next `call` to run a fresh handshake — a handshake failure
            //     surfaces as an error from `svc.call` (not the inner
            //     `poll_ready`), which we propagate.
            loop {
                std::future::poll_fn(|cx| svc.poll_ready(cx))
                    .await
                    .map_err(Into::into)?;
                let mut conn = svc.call(uri.clone()).await.map_err(Into::into)?;
                if std::future::poll_fn(|cx| conn.poll_ready(cx)).await.is_ok() {
                    return Ok(Box::new(TypedConnection(conn)) as BoxedConnection);
                }
                // drop `conn` → triggers pool cleanup (H1: discard via
                // CachedConnection::Drop; H2: Singleton already cleared state).
            }
        })
    }
}

/// Concrete Connection wrapping a Negotiated service.
struct TypedConnection<S>(S);

impl<S> Connection for TypedConnection<S>
where
    S: Service<http_1x::Request<SdkBody>> + Send + 'static,
    S::Response: Into<http_1x::Response<hyper::body::Incoming>>,
    S::Error: Into<BoxError>,
    S::Future: Send + 'static,
{
    fn send(
        self: Box<Self>,
        req: http_1x::Request<SdkBody>,
    ) -> BoxFuture<Result<http_1x::Response<hyper::body::Incoming>, BoxError>> {
        let mut svc = self.0;
        Box::pin(async move {
            std::future::poll_fn(|cx| svc.poll_ready(cx))
                .await
                .map_err(Into::into)?;
            svc.call(req).await.map(Into::into).map_err(Into::into)
        })
    }
}

/// A connection checked out from the H1 cache.
///
/// See `connection::CachedConnection`.
pub(crate) use connection::CachedConnection;

/// A connection checked out from the H2 singleton.
///
/// See `connection::SingletonConnection`.
pub(crate) use connection::SingletonConnection;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_key() {
        let uri: http_1x::Uri = "http://example.com:8080/path".parse().unwrap();
        let key = PoolKey::from_uri(&uri).unwrap();
        assert_eq!(key.authority.as_str(), "example.com:8080");
    }

    #[test]
    fn test_pool_key_missing_scheme() {
        let uri: http_1x::Uri = "/path".parse().unwrap();
        assert!(PoolKey::from_uri(&uri).is_none());
    }
}
