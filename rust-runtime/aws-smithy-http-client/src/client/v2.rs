/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! V2 HTTP client.
//!
//! Entry point: [`BuilderV2::new`] or [`Builder::new_v2`](super::Builder::new_v2).

pub use super::pool::connection::{
    Authority, CloseReason, ConnectionClosedEvent, ConnectionCreatedEvent, ConnectionEventListener,
    ConnectionFailedEvent, ConnectionReusedEvent, NegotiatedProtocol,
};

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use aws_smithy_runtime_api::client::connection::CaptureSmithyConnection;
use aws_smithy_runtime_api::client::connector_metadata::ConnectorMetadata;
use aws_smithy_runtime_api::client::http::{
    HttpClient, HttpConnector, HttpConnectorFuture, HttpConnectorSettings, SharedHttpClient,
    SharedHttpConnector,
};
use aws_smithy_runtime_api::client::orchestrator::{HttpRequest, HttpResponse};
use aws_smithy_runtime_api::client::result::ConnectorError;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_runtime_api::shared::IntoShared;
use hyper_util::client::legacy::connect::HttpConnector as HyperHttpConnector;

use super::downcast_error;
use super::pool::{self, ConnectionPool, PoolConfig};
use super::tls;
use super::{TlsProviderSelected, TlsUnset};
use crate::tls::TlsContext;

/// Builder for the v2 HTTP client.
///
/// Provides a flat, unified configuration surface for connection management,
/// TLS, timeouts, and pool behavior (all knobs in one place).
///
/// # Examples
///
/// ```no_run
/// use aws_smithy_http_client::v2::BuilderV2;
/// use std::time::Duration;
///
/// let client = BuilderV2::new()
///     .pool_idle_timeout(Duration::from_secs(20))
///     .build_http();
/// ```
#[derive(Clone)]
pub struct BuilderV2<Tls = TlsUnset> {
    pool_idle_timeout: Option<Duration>,
    tcp_nodelay: bool,
    max_connections: Option<usize>,
    max_connections_per_host: Option<usize>,
    proxy_config: Option<super::proxy::ProxyConfig>,
    connection_event_listener: Option<Arc<dyn super::pool::connection::ConnectionEventListener>>,
    tls: Tls,
}

impl<Tls: std::fmt::Debug> std::fmt::Debug for BuilderV2<Tls> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuilderV2")
            .field("pool_idle_timeout", &self.pool_idle_timeout)
            .field("tcp_nodelay", &self.tcp_nodelay)
            .field("max_connections", &self.max_connections)
            .field("max_connections_per_host", &self.max_connections_per_host)
            .field("proxy_config", &self.proxy_config)
            .field(
                "connection_event_listener",
                &self.connection_event_listener.as_ref().map(|_| ".."),
            )
            .finish()
    }
}

impl Default for BuilderV2<TlsUnset> {
    fn default() -> Self {
        Self {
            pool_idle_timeout: None,
            tcp_nodelay: true,
            max_connections: None,
            max_connections_per_host: None,
            proxy_config: None,
            connection_event_listener: None,
            tls: TlsUnset {},
        }
    }
}

// Methods available in any TLS state.
impl<Tls> BuilderV2<Tls> {
    /// Set the pool idle timeout.
    ///
    /// Connections idle longer than this duration are evicted from the pool.
    /// Set below the server's idle timeout to avoid stale connection errors
    /// (e.g., ~20s for S3).
    pub fn pool_idle_timeout(mut self, timeout: Duration) -> Self {
        self.pool_idle_timeout = Some(timeout);
        self
    }

    /// Set TCP_NODELAY on connections. Default: `true`.
    pub fn tcp_nodelay(mut self, nodelay: bool) -> Self {
        self.tcp_nodelay = nodelay;
        self
    }

    /// Set the maximum number of concurrent connections.
    ///
    /// Limits concurrent connection handshakes across all hosts. Cached
    /// (reused) connections do not count against this limit. When the limit
    /// is reached, new connection attempts wait until an existing handshake
    /// completes.
    pub fn max_connections(mut self, n: usize) -> Self {
        self.max_connections = Some(n);
        self
    }

    /// Set the maximum number of concurrent connections per host.
    ///
    /// Each unique (scheme, authority) pair has an independent connection
    /// budget. This limit is independent of `max_connections`; both can
    /// be set and are enforced simultaneously.
    pub fn max_connections_per_host(mut self, n: usize) -> Self {
        self.max_connections_per_host = Some(n);
        self
    }

    /// Configure proxy support.
    ///
    /// Threads `config` through the connector chain: HTTP/HTTPS proxies
    /// route requests via the proxy server; HTTPS-over-proxy uses
    /// `CONNECT` tunneling. The per-host proxy decision is stable for
    /// this builder's lifetime: all connections to a given authority
    /// go through the same path.
    ///
    /// To use environment variables (`HTTP_PROXY`, `HTTPS_PROXY`,
    /// `NO_PROXY`), pass `ProxyConfig::from_env()`.
    pub fn proxy_config(mut self, config: super::proxy::ProxyConfig) -> Self {
        self.proxy_config = Some(config);
        self
    }

    /// Mutable-reference variant of [`proxy_config`](Self::proxy_config).
    pub fn set_proxy_config(&mut self, config: Option<super::proxy::ProxyConfig>) -> &mut Self {
        self.proxy_config = config;
        self
    }

    /// Set a listener for connection lifecycle events (created, reused, closed, failed).
    pub fn connection_event_listener(
        mut self,
        listener: Arc<dyn super::pool::connection::ConnectionEventListener>,
    ) -> Self {
        self.connection_event_listener = Some(listener);
        self
    }

    /// Mutable-reference variant of [`connection_event_listener`](Self::connection_event_listener).
    pub fn set_connection_event_listener(
        &mut self,
        listener: Option<Arc<dyn super::pool::connection::ConnectionEventListener>>,
    ) -> &mut Self {
        self.connection_event_listener = listener;
        self
    }
}

impl BuilderV2<TlsUnset> {
    /// Create a new v2 client builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the TLS implementation.
    pub fn tls_provider(self, provider: tls::Provider) -> BuilderV2<TlsProviderSelected> {
        BuilderV2 {
            pool_idle_timeout: self.pool_idle_timeout,
            tcp_nodelay: self.tcp_nodelay,
            max_connections: self.max_connections,
            max_connections_per_host: self.max_connections_per_host,
            proxy_config: self.proxy_config,
            connection_event_listener: self.connection_event_listener,
            tls: TlsProviderSelected {
                provider,
                context: TlsContext::default(),
            },
        }
    }

    /// Build an HTTP client without TLS.
    #[doc(hidden)]
    pub fn build_http(self) -> SharedHttpClient {
        let mut tcp = HyperHttpConnector::new();
        tcp.set_nodelay(self.tcp_nodelay);
        let config = PoolConfig {
            max_connections: self.max_connections,
            max_connections_per_host: self.max_connections_per_host,
            pool_idle_timeout: self.pool_idle_timeout,
            connection_event_listener: self.connection_event_listener.clone(),
        };
        let proxy_matcher = proxy_matcher_from(&self.proxy_config);
        let pool = build_http_pool_with_proxy(tcp, &self.proxy_config, config);
        V2HttpClient::new(pool, proxy_matcher).into_shared()
    }

    /// Build an HTTP client without TLS, using a custom DNS resolver.
    ///
    /// Mirrors `build_http` but threads a custom `ResolveDns`. Intended
    /// for tests that use `WireMockServer::dns_resolver()` to redirect
    /// all lookups to a loopback mock. Public API for non-TLS resolver
    /// plumbing is `#[doc(hidden)]`; TLS is the common case.
    #[doc(hidden)]
    pub fn build_http_with_resolver(
        self,
        resolver: impl aws_smithy_runtime_api::client::dns::ResolveDns + Clone + 'static,
    ) -> SharedHttpClient {
        use crate::client::dns::HyperUtilResolver;
        let mut tcp = HyperHttpConnector::new_with_resolver(HyperUtilResolver { resolver });
        tcp.set_nodelay(self.tcp_nodelay);
        let config = PoolConfig {
            max_connections: self.max_connections,
            max_connections_per_host: self.max_connections_per_host,
            pool_idle_timeout: self.pool_idle_timeout,
            connection_event_listener: self.connection_event_listener.clone(),
        };
        let proxy_matcher = proxy_matcher_from(&self.proxy_config);
        let pool = build_http_pool_with_proxy(tcp, &self.proxy_config, config);
        V2HttpClient::new(pool, proxy_matcher).into_shared()
    }
}

/// Build a no-TLS pool that honors `proxy_config`. Wraps the TCP connector
/// with an HTTP proxy connector when configured. Emits a warning if an
/// HTTPS proxy is set without a TLS provider; connections to such a
/// proxy will fail at handshake time.
fn build_http_pool_with_proxy<R>(
    tcp: HyperHttpConnector<R>,
    proxy_config: &Option<super::proxy::ProxyConfig>,
    config: PoolConfig,
) -> ConnectionPool
where
    R: Clone + Send + Sync + 'static,
    R: tower::Service<hyper_util::client::legacy::connect::dns::Name>,
    R::Response: Iterator<Item = std::net::SocketAddr>,
    R::Future: Send,
    R::Error: Into<aws_smithy_runtime_api::box_error::BoxError>,
{
    use super::proxy;

    let proxy_config = proxy_config
        .clone()
        .unwrap_or_else(proxy::ProxyConfig::disabled);

    if proxy_config.requires_tls() {
        tracing::warn!(
            "HTTPS proxy configured but no TLS provider set. \
             Connections to HTTPS proxy servers will fail. \
             Consider configuring a TLS provider to enable TLS support."
        );
    }

    if proxy_config.is_disabled() {
        pool::build_pool(tcp, config)
    } else {
        let connector = super::connect::HttpProxyConnector::new(tcp, proxy_config);
        pool::build_pool(connector, config)
    }
}

impl BuilderV2<TlsProviderSelected> {
    /// Set the TLS context (custom trust store, etc.).
    pub fn tls_context(mut self, context: TlsContext) -> Self {
        self.tls.context = context;
        self
    }

    /// Build an HTTPS client with the selected TLS provider.
    pub fn build_https(self) -> SharedHttpClient {
        let mut tcp = HyperHttpConnector::new();
        tcp.set_nodelay(self.tcp_nodelay);
        tcp.enforce_http(false);
        self.build_from_tcp(tcp)
    }

    /// Build an HTTPS client using a custom DNS resolver.
    pub fn build_with_resolver(
        self,
        resolver: impl aws_smithy_runtime_api::client::dns::ResolveDns + Clone + 'static,
    ) -> SharedHttpClient {
        use crate::client::dns::HyperUtilResolver;
        let mut tcp = HyperHttpConnector::new_with_resolver(HyperUtilResolver { resolver });
        tcp.set_nodelay(self.tcp_nodelay);
        tcp.enforce_http(false);
        self.build_from_tcp(tcp)
    }

    fn build_from_tcp<R>(self, tcp: HyperHttpConnector<R>) -> SharedHttpClient
    where
        R: Clone + Send + Sync + 'static,
        R: tower::Service<hyper_util::client::legacy::connect::dns::Name>,
        R::Response: Iterator<Item = std::net::SocketAddr>,
        R::Future: Send,
        R::Error: Into<aws_smithy_runtime_api::box_error::BoxError>,
    {
        use super::proxy;

        let config = PoolConfig {
            max_connections: self.max_connections,
            max_connections_per_host: self.max_connections_per_host,
            pool_idle_timeout: self.pool_idle_timeout,
            connection_event_listener: self.connection_event_listener.clone(),
        };

        let proxy_config = self
            .proxy_config
            .clone()
            .unwrap_or_else(proxy::ProxyConfig::disabled);
        let proxy_matcher = proxy_matcher_from(&self.proxy_config);

        match &self.tls.provider {
            #[cfg(any(
                feature = "rustls-aws-lc",
                feature = "rustls-aws-lc-fips",
                feature = "rustls-ring"
            ))]
            tls::Provider::Rustls(crypto_mode) => {
                let connector = tls::rustls_provider::build_connector::wrap_connector(
                    tcp,
                    crypto_mode.clone(),
                    &self.tls.context,
                    proxy_config,
                );
                let pool = pool::build_pool(connector, config);
                V2HttpClient::new(pool, proxy_matcher).into_shared()
            }
            #[cfg(feature = "s2n-tls")]
            tls::Provider::S2nTls => {
                let connector = tls::s2n_tls_provider::build_connector::wrap_connector(
                    tcp,
                    &self.tls.context,
                    proxy_config,
                );
                let pool = pool::build_pool(connector, config);
                V2HttpClient::new(pool, proxy_matcher).into_shared()
            }
            // Provider is #[non_exhaustive]; this arm is unreachable when any
            // TLS feature is enabled (which is required to construct a Provider).
            #[allow(unreachable_patterns)]
            _ => unreachable!("a TLS feature must be enabled to use build_https()"),
        }
    }
}

// ---------------------------------------------------------------------------
// HttpConnector adapter
// ---------------------------------------------------------------------------

/// Smithy `HttpConnector` backed by the v2 connection pool.
///
/// Constructed fresh per `HttpClient::http_connector` call so it can capture
/// the per-operation `HttpConnectorSettings` (connect/read timeouts). The pool
/// itself is shared across all operations via `Arc`.
struct PooledConnector {
    pool: Arc<ConnectionPool>,
    connect_timeout: Option<Duration>,
    read_timeout: Option<Duration>,
    /// Present when any timeout is configured. Required to construct
    /// `Timeout` futures. `V2HttpClient::http_connector` panics if
    /// a timeout is set but `sleep_impl` is absent (matches v1).
    sleep_impl: Option<aws_smithy_async::rt::sleep::SharedAsyncSleep>,
    /// Proxy URL matcher, cloned from `V2HttpClient`. Drives the
    /// `Proxy-Authorization` header injection in `call`.
    proxy_matcher: Option<Arc<hyper_util::client::proxy::matcher::Matcher>>,
}

impl std::fmt::Debug for PooledConnector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PooledConnector").finish()
    }
}

impl HttpConnector for PooledConnector {
    fn call(&self, request: HttpRequest) -> HttpConnectorFuture {
        let pool = self.pool.clone();
        let connect_timeout = self.connect_timeout;
        let read_timeout = self.read_timeout;
        let sleep_impl = self.sleep_impl.clone();
        let proxy_matcher = self.proxy_matcher.clone();
        HttpConnectorFuture::new(async move {
            let mut request = request
                .try_into_http1x()
                .map_err(|err| ConnectorError::user(err.into()))?;

            // Save the full URI for pool routing before rewriting to origin-form.
            let full_uri = request.uri().clone();

            // Inject Proxy-Authorization for HTTP-through-proxy requests.
            // Shared with v1 via [`super::proxy::add_proxy_auth_header`];
            // HTTPS-through-proxy uses CONNECT tunneling and authenticates
            // inside the tunnel-establishment exchange, not via this header.
            if let Some(matcher) = proxy_matcher.as_ref() {
                super::proxy::add_proxy_auth_header(&mut request, matcher);
            }

            // Create a request-scoped capture the pool will fill in with
            // the `ConnectionMetadata` for the selected connection, and
            // wire `CaptureSmithyConnection` to read from it. When the
            // runtime's connection-poisoning interceptor later calls
            // `ConnectionMetadata::poison()`, it flips the `PoisonPill` on
            // the actual `ManagedConnection` selected for this request.
            if let Some(capture_smithy) = request.extensions().get::<CaptureSmithyConnection>() {
                let capture = pool::ConnectionMetadataCapture::new();
                let for_retriever = capture.clone();
                capture_smithy.set_connection_retriever(move || for_retriever.get());
                request.extensions_mut().insert(capture);
            }

            // Attach the per-op read timeout hint (if any) as a request
            // extension. `H{1,2}Checkout::call` reads it and wraps the
            // request dispatch: bounds request-write + response-headers,
            // nothing else.
            if let Some((duration, sleep)) = read_timeout.zip(sleep_impl.clone()) {
                request
                    .extensions_mut()
                    .insert(pool::ReadTimeoutHint(pool::TimeoutContext::new(
                        duration, sleep,
                    )));
            }

            // Set Host header from the URI authority if not already present.
            if !request.headers().contains_key(http_1x::header::HOST) {
                if let Some(authority) = full_uri.authority() {
                    let host = match get_non_default_port(&full_uri) {
                        Some(port) => format!("{}:{}", authority.host(), port),
                        None => authority.host().to_string(),
                    };
                    request.headers_mut().insert(
                        http_1x::header::HOST,
                        http_1x::HeaderValue::from_str(&host)
                            .expect("authority is valid header value"),
                    );
                }
            }

            // URI request-target rewriting happens in `H1Checkout::call`
            // because the correct form depends on whether the connection
            // landed on a proxy (absolute-form) or direct origin
            // (origin-form). H2 uses pseudo-headers derived by hyper from
            // the absolute URI, so it doesn't need rewriting here.

            // Build the connect context: routing URI + connect timeout.
            // Cache hits skip the connector entirely so connect_timeout is
            // automatically new-connection-only.
            let connect_ctx = pool::ConnectCtx::new(
                full_uri,
                connect_timeout
                    .zip(sleep_impl)
                    .map(|(d, s)| pool::TimeoutContext::new(d, s)),
            );

            let response = pool
                .send_request(connect_ctx, request)
                .await
                .map_err(downcast_error)?;

            HttpResponse::try_from(response).map_err(|err| ConnectorError::other(err.into(), None))
        })
    }
}

// ---------------------------------------------------------------------------
// HttpClient impl
// ---------------------------------------------------------------------------

/// Build the proxy URL matcher from a `ProxyConfig`, returning `None` when
/// no proxy is configured. Wrapped in `Arc` so each `PooledConnector`
/// constructed by `http_connector` can share the (immutable) matcher
/// without per-call allocation.
fn proxy_matcher_from(
    proxy_config: &Option<super::proxy::ProxyConfig>,
) -> Option<Arc<hyper_util::client::proxy::matcher::Matcher>> {
    proxy_config
        .as_ref()
        .map(|c| Arc::new(c.clone().into_hyper_util_matcher()))
}

/// Returns the port if it is not the default for the scheme.
fn get_non_default_port(uri: &http_1x::Uri) -> Option<http_1x::uri::Port<&str>> {
    match (uri.port().map(|p| p.as_u16()), uri.scheme()) {
        (Some(443), Some(s)) if *s == http_1x::uri::Scheme::HTTPS => None,
        (Some(80), Some(s)) if *s == http_1x::uri::Scheme::HTTP => None,
        _ => uri.port(),
    }
}

/// Smithy `HttpClient` backed by the v2 connection pool.
struct V2HttpClient {
    pool: Arc<ConnectionPool>,
    /// Proxy URL matcher, present iff the builder was configured with a
    /// `proxy_config`. Used by `PooledConnector` to inject
    /// `Proxy-Authorization` headers on HTTP-through-proxy requests
    /// (HTTPS-CONNECT auth is handled by the proxy connector itself
    /// during tunnel setup).
    proxy_matcher: Option<Arc<hyper_util::client::proxy::matcher::Matcher>>,
}

impl std::fmt::Debug for V2HttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("V2HttpClient").finish()
    }
}

impl V2HttpClient {
    fn new(
        pool: ConnectionPool,
        proxy_matcher: Option<Arc<hyper_util::client::proxy::matcher::Matcher>>,
    ) -> Self {
        Self {
            pool: Arc::new(pool),
            proxy_matcher,
        }
    }
}

impl HttpClient for V2HttpClient {
    fn http_connector(
        &self,
        settings: &HttpConnectorSettings,
        components: &RuntimeComponents,
    ) -> SharedHttpConnector {
        let connect_timeout = settings.connect_timeout();
        let read_timeout = settings.read_timeout();
        let sleep_impl = components.sleep_impl();

        if (connect_timeout.is_some() || read_timeout.is_some()) && sleep_impl.is_none() {
            panic!(
                "an async sleep impl is required to use connect/read timeouts with \
                 the v2 HTTP client; provide one via `RuntimeComponents::sleep_impl`"
            );
        }

        SharedHttpConnector::new(PooledConnector {
            pool: self.pool.clone(),
            connect_timeout,
            read_timeout,
            sleep_impl,
            proxy_matcher: self.proxy_matcher.clone(),
        })
    }

    fn connector_metadata(&self) -> Option<ConnectorMetadata> {
        // TODO: consider distinguishing v1 vs v2 clients and including TLS
        // provider info (e.g. "rustls"/"0.23") in the connector metadata.
        Some(ConnectorMetadata::new("hyper", Some(Cow::Borrowed("1.x"))))
    }
}
