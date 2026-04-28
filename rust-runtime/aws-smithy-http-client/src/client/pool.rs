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
use std::task::Poll;

use aws_smithy_types::body::SdkBody;
use hyper_util::client::legacy::connect::Connection as HyperConnection;
use hyper_util::client::pool as hpool;
use hyper_util::rt::TokioExecutor;
use tower::{Service, ServiceExt};

use connection::{
    CachedConnection, CheckoutResponse, ConnectionGuard, GuardedBody, ManagedConnection,
    SingletonConnection,
};
use handshake::{H1ConnectAndHandshake, H1SendRequest, H2ConnectAndHandshake, H2SendRequest};

pub(crate) type BoxError = Box<dyn std::error::Error + Send + Sync>;
type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Pool-level configuration.
///
/// Plumbed from the eventual `Builder::new_v2()` public API through
/// `build_pool` to `ConnectionPool`. Internal type — the public surface
/// is the builder's per-knob setters, not this struct.
///
/// All fields are `None` by default (defaults applied by the pool where
/// they take effect, not here). Fields are `pub(crate)` because
/// consumers of this struct are all within the pool module; accessor
/// methods would add noise without benefit.
#[derive(Clone, Debug, Default)]
pub(crate) struct PoolConfig {
    /// Upper bound on concurrent connections (total, across all hosts).
    /// Enforced via `tower::ConcurrencyLimit` at the connection
    /// establishment layer. `None` = unlimited.
    pub(crate) max_connections: Option<usize>,

    /// How long an idle connection may stay in the pool before being
    /// evicted. `None` = no eviction (hyper-util's default behavior).
    pub(crate) pool_idle_timeout: Option<std::time::Duration>,
}

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

/// Build a connection pool for the given connector and configuration.
pub(crate) fn build_pool<C, IO>(connector: C, config: PoolConfig) -> ConnectionPool
where
    C: Service<http_1x::Uri, Response = IO> + Clone + Send + Sync + 'static,
    C::Error: Into<BoxError> + 'static,
    C::Future: Unpin + Send + 'static,
    IO: hyper::rt::Read + hyper::rt::Write + HyperConnection + Unpin + Send + 'static,
{
    let make_entry = move |_uri: &http_1x::Uri| -> Box<dyn PoolEntry> {
        // TODO: consult `config.max_connections` here by wrapping the
        //   per-host connector with `tower::ConcurrencyLimit`, and consult
        //   `config.pool_idle_timeout` by driving `retain()` on a background
        //   task. Currently the config is stored on the pool but not yet
        //   applied.
        let stack = hpool::negotiate::builder()
            .connect(connector.clone())
            .inspect(|conn: &IO| conn.connected().is_negotiated_h2())
            .fallback(tower::layer::layer_fn(|inspector| {
                cache::builder()
                    .executor(TokioExecutor::new())
                    .build(H1ConnectAndHandshake::new(inspector))
                    .map_response(|cached| H1Checkout::new(CachedConnection::new(cached)))
            }))
            .upgrade(tower::layer::layer_fn(|inspected| {
                hpool::singleton::Singleton::new(H2ConnectAndHandshake::new(inspected))
                    .map_response(|singled| H2Checkout::new(SingletonConnection::new(singled)))
            }))
            .build();
        Box::new(TypedPoolEntry(stack))
    };

    ConnectionPool {
        config,
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
    /// Pool-wide configuration. Stored for consultation at request time
    /// and for re-use if `make_entry` needs config (not yet wired).
    #[allow(dead_code)]
    config: PoolConfig,
    hosts: Mutex<HashMap<PoolKey, Box<dyn PoolEntry>>>,
    make_entry: Box<dyn Fn(&http_1x::Uri) -> Box<dyn PoolEntry> + Send + Sync>,
}

impl ConnectionPool {
    /// Send a request through the pool.
    ///
    /// Routes to the appropriate per-host pool stack and sends the request.
    pub(crate) async fn send_request(
        &self,
        uri: http_1x::Uri,
        req: http_1x::Request<SdkBody>,
    ) -> Result<http_1x::Response<SdkBody>, BoxError> {
        let key = PoolKey::from_uri(&uri).ok_or("request URI must have scheme and authority")?;

        // Dispatch the request through the per-host entry. The lock is
        // held only long enough to look up / create the entry and call
        // `send` — all I/O happens in the returned future, outside the
        // lock.
        let fut = {
            let mut hosts = self.hosts.lock().unwrap();
            if !hosts.contains_key(&key) {
                hosts.insert(key.clone(), (self.make_entry)(&uri));
            }
            hosts.get_mut(&key).unwrap().send(uri, req)
        };

        fut.await
    }
}

/// Tower `Service` wrapper for an H1 pool checkout.
///
/// Produced by the H1 fallback leg (one per checkout from the cache).
/// Its `Service::call(req)` consumes the held `CachedConnection`, runs
/// the request, and wraps the response body so the connection returns
/// to the pool only when the body is fully drained.
///
/// # Single-use
///
/// Each checkout represents one `CachedConnection`. `call` moves the
/// connection into the response future via `Option::take()`; calling
/// `call` twice panics (unreachable given our checkout-per-request flow).
///
/// The `UnusedH2Phantom` generic is a phantom type parameter: the H1 path
/// never populates the H2 variant of `ConnectionGuard` and never holds an
/// H2 checkout, but both legs must produce the same `CheckoutResponse<…>`
/// so `Negotiate` can compose them uniformly. The phantom slot here is
/// whatever type the H2 leg's `SingletonConnection` holds, resolved by
/// type inference at the pool composition site — this wrapper ignores it.
pub(crate) struct H1Checkout<UnusedH2Phantom> {
    conn: Option<CachedConnection<H1SendRequest>>,
    _marker: std::marker::PhantomData<fn() -> UnusedH2Phantom>,
}

impl<UnusedH2Phantom> H1Checkout<UnusedH2Phantom> {
    pub(crate) fn new(conn: CachedConnection<H1SendRequest>) -> Self {
        Self {
            conn: Some(conn),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<UnusedH2Phantom> Service<http_1x::Request<SdkBody>> for H1Checkout<UnusedH2Phantom> {
    type Response = CheckoutResponse<UnusedH2Phantom>;
    type Error = BoxError;
    type Future = BoxFuture<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.conn
            .as_mut()
            .expect("H1Checkout::poll_ready after call")
            .poll_ready(cx)
    }

    fn call(&mut self, req: http_1x::Request<SdkBody>) -> Self::Future {
        let mut conn = self.conn.take().expect("H1Checkout::call called twice");
        Box::pin(async move {
            let resp = conn.call(req).await?;
            let (parts, body) = resp.into_parts();
            let body = GuardedBody::new(body, ConnectionGuard::H1(conn));
            Ok(http_1x::Response::from_parts(parts, body))
        })
    }
}

/// Tower `Service` wrapper for an H2 pool checkout.
///
/// Symmetric to `H1Checkout`. The `Singleton` type parameter is the
/// `SingletonConnection<T>` inner `T`, which at the composition site
/// resolves to the unnameable `hyper_util::client::pool::singleton::
/// Singled<ManagedConnection<H2SendRequest>>`. We carry it through
/// generics and never name it concretely.
///
/// # Single-use
///
/// Same contract as `H1Checkout`.
pub(crate) struct H2Checkout<Singleton> {
    conn: Option<SingletonConnection<Singleton>>,
}

impl<Singleton> H2Checkout<Singleton> {
    pub(crate) fn new(conn: SingletonConnection<Singleton>) -> Self {
        Self { conn: Some(conn) }
    }
}

impl<Singleton> Service<http_1x::Request<SdkBody>> for H2Checkout<Singleton>
where
    Singleton: Service<http_1x::Request<SdkBody>, Response = http_1x::Response<hyper::body::Incoming>>
        + Send
        + 'static,
    Singleton::Error: Into<BoxError>,
    Singleton::Future: Send + 'static,
{
    type Response = CheckoutResponse<Singleton>;
    type Error = BoxError;
    type Future = BoxFuture<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.conn
            .as_mut()
            .expect("H2Checkout::poll_ready after call")
            .poll_ready(cx)
            .map_err(Into::into)
    }

    fn call(&mut self, req: http_1x::Request<SdkBody>) -> Self::Future {
        let mut conn = self.conn.take().expect("H2Checkout::call called twice");
        Box::pin(async move {
            let resp = conn.call(req).await.map_err(Into::into)?;
            let (parts, body) = resp.into_parts();
            let body = GuardedBody::new(body, ConnectionGuard::H2(conn));
            Ok(http_1x::Response::from_parts(parts, body))
        })
    }
}

/// Type-erased per-host pool entry.
///
/// Each host has one of these, wrapping the unnameable Negotiate stack.
/// Dyn erasure is structural: hosts have different Negotiate compositions
/// with different unnameable inner types, so `ConnectionPool::hosts` can't
/// hold a concrete type.
///
/// The entry is request-granular — `send` performs checkout + health
/// filter + dispatch + body-guard setup atomically for one request. This
/// folds what was previously a separate `acquire(uri) -> Connection`
/// followed by `connection.send(req)` into one call.
///
/// # Future: splitting acquire and send
///
/// If we later need to expose a checked-out connection handle to callers
/// (e.g. connection warming, request pipelining on a single checkout,
/// or pool-external reuse), reintroduce a separate `Connection` trait
/// and split this into `acquire(uri) -> Box<dyn Connection>` plus
/// `Connection::send(self: Box<Self>, req) -> Response`. The current
/// shape collapses that split because no caller needs it today.
trait PoolEntry: Send + Sync {
    fn send(
        &mut self,
        uri: http_1x::Uri,
        req: http_1x::Request<SdkBody>,
    ) -> BoxFuture<Result<http_1x::Response<SdkBody>, BoxError>>;
}

/// Concrete PoolEntry wrapping a Negotiate stack.
///
/// `PoolUnnameable` propagates whatever pool-internal unnameable type
/// flows through the checkout services' `CheckoutResponse`; it's carried
/// through so `Conn::Response` can name `CheckoutResponse<PoolUnnameable>`.
struct TypedPoolEntry<S>(S);

impl<S, Conn, PoolUnnameable> PoolEntry for TypedPoolEntry<S>
where
    S: Service<http_1x::Uri, Response = Conn> + Clone + Send + Sync + 'static,
    S::Error: Into<BoxError> + 'static,
    S::Future: Send + 'static,
    Conn: Service<http_1x::Request<SdkBody>, Response = CheckoutResponse<PoolUnnameable>>
        + Send
        + 'static,
    Conn::Error: Into<BoxError>,
    Conn::Future: Send + 'static,
    PoolUnnameable: Send + Sync + 'static,
{
    fn send(
        &mut self,
        uri: http_1x::Uri,
        req: http_1x::Request<SdkBody>,
    ) -> BoxFuture<Result<http_1x::Response<SdkBody>, BoxError>> {
        let mut svc = self.0.clone();
        Box::pin(async move {
            // Checkout loop. Pops past stale idle entries (poisoned or dead).
            // Converges because:
            //   - Cache idle set is bounded; each post-checkout `poll_ready`
            //     Err flips `is_closed` so `Cached::Drop` skips reinsertion,
            //     shrinking the set by one.
            //   - Singleton clears to `Empty` on `poll_ready` Err, forcing
            //     the next `call` to run a fresh handshake — a handshake
            //     failure surfaces as an error from `svc.call` (not the
            //     inner `poll_ready`), which we propagate.
            let mut conn = loop {
                std::future::poll_fn(|cx| svc.poll_ready(cx))
                    .await
                    .map_err(Into::into)?;
                let mut checkout = svc.call(uri.clone()).await.map_err(Into::into)?;
                if std::future::poll_fn(|cx| checkout.poll_ready(cx))
                    .await
                    .is_ok()
                {
                    break checkout;
                }
                // drop `checkout` → triggers pool cleanup (H1: discard via
                // CachedConnection::Drop; H2: Singleton already cleared
                // state).
            };

            // Dispatch the request; convert the body into `SdkBody` at the
            // boundary so consumers (and `dyn PoolEntry`) don't see our
            // internal `GuardedBody<...>` type. The guard inside
            // `GuardedBody` lives on inside `SdkBody` until the body is
            // fully drained.
            let resp = conn.call(req).await.map_err(Into::into)?;
            let (parts, body) = resp.into_parts();
            Ok(http_1x::Response::from_parts(
                parts,
                SdkBody::from_body_1_x(body),
            ))
        })
    }
}

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
