/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! V2 HTTP client.
//!
//! Entry point: [`BuilderV2::new`] or [`Builder::new_v2`](super::Builder::new_v2).

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use aws_smithy_runtime_api::client::connection::CaptureSmithyConnection;
use aws_smithy_runtime_api::client::connection::ConnectionMetadata;
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
/// TLS, timeouts, and pool behavior — all knobs in one place.
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
#[derive(Clone, Debug)]
pub struct BuilderV2<Tls = TlsUnset> {
    pool_idle_timeout: Option<Duration>,
    tcp_nodelay: bool,
    max_connections: Option<usize>,
    max_connections_per_host: Option<usize>,
    tls: Tls,
}

impl Default for BuilderV2<TlsUnset> {
    fn default() -> Self {
        Self {
            pool_idle_timeout: None,
            tcp_nodelay: true,
            max_connections: None,
            max_connections_per_host: None,
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
    /// budget. This limit is independent of `max_connections` — both can
    /// be set and are enforced simultaneously.
    pub fn max_connections_per_host(mut self, n: usize) -> Self {
        self.max_connections_per_host = Some(n);
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
        };
        let pool = pool::build_pool(tcp, config);
        V2HttpClient::new(pool).into_shared()
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
        R::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        use super::proxy;

        let config = PoolConfig {
            max_connections: self.max_connections,
            max_connections_per_host: self.max_connections_per_host,
            pool_idle_timeout: self.pool_idle_timeout,
        };

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
                    proxy::ProxyConfig::disabled(),
                );
                let pool = pool::build_pool(connector, config);
                V2HttpClient::new(pool).into_shared()
            }
            #[cfg(feature = "s2n-tls")]
            tls::Provider::S2nTls => {
                let connector = tls::s2n_tls_provider::build_connector::wrap_connector(
                    tcp,
                    &self.tls.context,
                    proxy::ProxyConfig::disabled(),
                );
                let pool = pool::build_pool(connector, config);
                V2HttpClient::new(pool).into_shared()
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
struct PooledConnector {
    pool: Arc<ConnectionPool>,
}

impl std::fmt::Debug for PooledConnector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PooledConnector").finish()
    }
}

impl HttpConnector for PooledConnector {
    fn call(&self, request: HttpRequest) -> HttpConnectorFuture {
        let pool = self.pool.clone();
        HttpConnectorFuture::new(async move {
            let mut request = request
                .try_into_http1x()
                .map_err(|err| ConnectorError::user(err.into()))?;

            // Save the full URI for pool routing before rewriting to origin-form.
            let full_uri = request.uri().clone();

            // Wire CaptureSmithyConnection with a no-op poison_fn so the interceptor
            // pipeline doesn't panic. TODO: wire real poison from ManagedConnection.
            if let Some(capture) = request.extensions().get::<CaptureSmithyConnection>() {
                capture.set_connection_retriever(|| {
                    Some(
                        ConnectionMetadata::builder()
                            .proxied(false)
                            .poison_fn(|| {})
                            .build(),
                    )
                });
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

            // Rewrite URI to origin-form for HTTP/1.1.
            let origin = match request.uri().path_and_query() {
                Some(pq) => {
                    let mut parts = http_1x::uri::Parts::default();
                    parts.path_and_query = Some(pq.clone());
                    http_1x::Uri::from_parts(parts).expect("path is valid uri")
                }
                None => http_1x::Uri::from_static("/"),
            };
            *request.uri_mut() = origin;

            let response = pool
                .send_request(full_uri, request)
                .await
                .map_err(downcast_error)?;

            HttpResponse::try_from(response).map_err(|err| ConnectorError::other(err.into(), None))
        })
    }
}

// ---------------------------------------------------------------------------
// HttpClient impl
// ---------------------------------------------------------------------------

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
}

impl std::fmt::Debug for V2HttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("V2HttpClient").finish()
    }
}

impl V2HttpClient {
    fn new(pool: ConnectionPool) -> Self {
        Self {
            pool: Arc::new(pool),
        }
    }
}

impl HttpClient for V2HttpClient {
    fn http_connector(
        &self,
        _settings: &HttpConnectorSettings,
        _components: &RuntimeComponents,
    ) -> SharedHttpConnector {
        SharedHttpConnector::new(PooledConnector {
            pool: self.pool.clone(),
        })
    }

    fn connector_metadata(&self) -> Option<ConnectorMetadata> {
        // TODO: consider distinguishing v1 vs v2 clients and including TLS
        // provider info (e.g. "rustls"/"0.23"). Revisit alongside 3f (TLS support).
        Some(ConnectorMetadata::new("hyper", Some(Cow::Borrowed("1.x"))))
    }
}
