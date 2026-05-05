/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Connection pool for the v2 HTTP client.
//!
//! Routes requests by `(scheme, authority)` to per-host pool stacks,
//! negotiates HTTP/1.1 vs HTTP/2 via ALPN, caches H1 connections for
//! reuse, multiplexes over a singleton H2 connection per host, enforces
//! total and per-host connection limits, and surfaces connection
//! metadata (remote/local address, poison handle) to the adapter layer
//! for the runtime's connection-poisoning interceptor.

mod connection;
mod handshake;
mod vendored_cache;

/// Connection-caching pool layer.
///
/// Re-exports [`vendored_cache`], our vendored copy of hyper-util's
/// `pool::cache` with SDK-specific modifications. The re-export insulates
/// callers from the vendoring detail — all pool code uses `cache::…`
/// regardless of where the implementation lives.
mod cache {
    pub(crate) use super::vendored_cache::*;
}

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Poll;

use aws_smithy_types::body::SdkBody;
use hyper_util::client::legacy::connect::Connection as HyperConnection;
use hyper_util::client::pool as hpool;
use hyper_util::rt::TokioExecutor;
use tokio::sync::Semaphore;
use tower::{Service, ServiceExt};

use connection::{
    CachedConnection, CheckoutResponse, ConnectionGuard, GuardedBody, H2ConnectionRef,
    SingletonConnection,
};
use handshake::{H1ConnectAndHandshake, H1SendRequest, H2ConnectAndHandshake};

pub(crate) type BoxError = Box<dyn std::error::Error + Send + Sync>;
type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Request-extension slot the pool checkout fills in with the
/// `ConnectionMetadata` for the selected connection.
///
/// Read later by the adapter's `CaptureSmithyConnection` retriever, which
/// is what `ConnectionPoisoningInterceptor` uses to decide whether to call
/// `ConnectionMetadata::poison()` on a transient error. Poisoning flips the
/// shared `PoisonPill` on the actual `ManagedConnection`, so the pool
/// skips it on checkout and drops it on return.
///
/// Write-once per request: `H{1,2}Checkout::call` sets it exactly once
/// during checkout. Subsequent sets are no-ops (the `OnceLock` guarantees
/// single-init). Retries produce fresh `HttpConnector::call` invocations
/// which create fresh capture slots — each attempt's metadata points at
/// the connection used for that attempt.
#[derive(Clone, Default)]
pub(crate) struct ConnectionMetadataCapture {
    slot: Arc<std::sync::OnceLock<aws_smithy_runtime_api::client::connection::ConnectionMetadata>>,
}

impl ConnectionMetadataCapture {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn set(
        &self,
        metadata: aws_smithy_runtime_api::client::connection::ConnectionMetadata,
    ) {
        // Silently ignore duplicate sets: single-set is the contract, extra
        // sets would only happen via pool-internal bugs.
        let _ = self.slot.set(metadata);
    }

    pub(crate) fn get(
        &self,
    ) -> Option<aws_smithy_runtime_api::client::connection::ConnectionMetadata> {
        self.slot.get().cloned()
    }
}

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
    /// Enforced via semaphore at the connection establishment layer.
    /// `None` = unlimited.
    pub(crate) max_connections: Option<usize>,

    /// Upper bound on concurrent connections per host.
    /// Each unique (scheme, authority) pair gets an independent semaphore.
    /// `None` = unlimited.
    pub(crate) max_connections_per_host: Option<usize>,

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
    let global_sem = config.max_connections.map(|n| Arc::new(Semaphore::new(n)));
    let max_per_host = config.max_connections_per_host;

    let make_entry = move |_uri: &http_1x::Uri| -> Box<dyn PoolEntry> {
        let per_host_sem = max_per_host.map(|n| Arc::new(Semaphore::new(n)));
        let limited =
            handshake::ConnectionLimit::new(connector.clone(), global_sem.clone(), per_host_sem);

        // Per-host bridge: `H2ConnectAndHandshake` publishes on each new
        // handshake; `SingletonConnection` reads the current entry at
        // checkout. Clones share the underlying slot.
        let h2_ref = H2ConnectionRef::new();

        let stack = hpool::negotiate::builder()
            .connect(limited)
            .inspect(
                |(conn, _permit): &(IO, Arc<connection::ConnectionPermit>)| {
                    conn.connected().is_negotiated_h2()
                },
            )
            .fallback(tower::layer::layer_fn(|inspector| {
                cache::builder()
                    .executor(TokioExecutor::new())
                    .build(H1ConnectAndHandshake::new(inspector))
                    .map_response(|cached| H1Checkout::new(CachedConnection::new(cached)))
            }))
            .upgrade({
                let h2_ref = h2_ref.clone();
                tower::layer::layer_fn(move |inspected| {
                    hpool::singleton::Singleton::new(H2ConnectAndHandshake::new(
                        inspected,
                        h2_ref.clone(),
                    ))
                    .map_response({
                        let h2_ref = h2_ref.clone();
                        move |singled| {
                            H2Checkout::new(SingletonConnection::new(singled, h2_ref.clone()))
                        }
                    })
                })
            })
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
    /// Pool-wide configuration.
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
        // Populate the adapter-provided capture so the
        // `CaptureSmithyConnection` retriever can return a live
        // `ConnectionMetadata` pointing at THIS connection's poison pill.
        if let Some(capture) = req.extensions().get::<ConnectionMetadataCapture>() {
            capture.set(conn.metadata());
        }
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
        // Populate the adapter-provided capture. The metadata is published
        // by `H2ConnectAndHandshake` when a fresh H2 connection is
        // established; if a request somehow reaches us before any metadata
        // is available we leave the capture empty (poison becomes a no-op
        // for that request, matching the capture-absent default).
        if let Some(capture) = req.extensions().get::<ConnectionMetadataCapture>() {
            if let Some(metadata) = conn.metadata() {
                capture.set(metadata);
            }
        }
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
/// `send` performs checkout, health filtering, request dispatch, and
/// body-guard setup atomically for a single request.
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
