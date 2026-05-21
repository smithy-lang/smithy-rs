/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Connection pool builder.
//!
//! Entry point: [`SharedPool::builder`](super::SharedPool::builder).

pub use super::connection::{
    Authority, CloseReason, ConnectionClosedEvent, ConnectionCreatedEvent, ConnectionEventListener,
    ConnectionFailedEvent, ConnectionReusedEvent, ConnectionTiming, NegotiatedProtocol,
};

use std::sync::Arc;
use std::time::Duration;

use hyper_util::client::legacy::connect::HttpConnector as HyperHttpConnector;

use super::{ConnectionPool, PoolConfig};
use crate::client::tls;
use crate::client::{TlsProviderSelected, TlsUnset};
use crate::tls::TlsContext;

/// Builder for the v2 HTTP client.
///
/// Provides a flat, unified configuration surface for connection management,
/// TLS, timeouts, and pool behavior (all knobs in one place).
///
/// # Examples
///
/// ```no_run
/// use aws_smithy_http_client::pool::{SharedPool, Client};
/// use std::time::Duration;
///
/// let pool = SharedPool::builder()
///     .pool_idle_timeout(Duration::from_secs(20))
///     .build_http();
/// let client = Client::new(&pool);
/// ```
#[derive(Clone)]
pub struct Builder<Tls = TlsUnset> {
    pool_idle_timeout: Option<Duration>,
    tcp_nodelay: bool,
    max_connections: Option<usize>,
    max_connections_per_host: Option<usize>,
    proxy_config: Option<crate::client::proxy::ProxyConfig>,
    connection_event_listener: Option<Arc<dyn super::connection::ConnectionEventListener>>,
    cross_partition_policy: super::partition::CrossPartitionPolicy,
    dns_resolver: Option<aws_smithy_runtime_api::client::dns::SharedDnsResolver>,
    tls: Tls,
}

impl<Tls: std::fmt::Debug> std::fmt::Debug for Builder<Tls> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Builder")
            .field("pool_idle_timeout", &self.pool_idle_timeout)
            .field("tcp_nodelay", &self.tcp_nodelay)
            .field("max_connections", &self.max_connections)
            .field("max_connections_per_host", &self.max_connections_per_host)
            .field("proxy_config", &self.proxy_config)
            .field(
                "connection_event_listener",
                &self.connection_event_listener.as_ref().map(|_| ".."),
            )
            .field("cross_partition_policy", &self.cross_partition_policy)
            .field("dns_resolver", &self.dns_resolver.as_ref().map(|_| ".."))
            .finish()
    }
}

impl Default for Builder<TlsUnset> {
    fn default() -> Self {
        Self {
            pool_idle_timeout: None,
            tcp_nodelay: true,
            max_connections: None,
            max_connections_per_host: None,
            proxy_config: None,
            connection_event_listener: None,
            cross_partition_policy: super::partition::CrossPartitionPolicy::default(),
            dns_resolver: None,
            tls: TlsUnset {},
        }
    }
}

// Methods available in any TLS state.
impl<Tls> Builder<Tls> {
    /// Set the pool idle timeout.
    ///
    /// Connections idle longer than this duration are evicted from the pool.
    /// Set below the server's idle timeout to avoid stale connection errors.
    pub fn pool_idle_timeout(mut self, timeout: Duration) -> Self {
        self.pool_idle_timeout = Some(timeout);
        self
    }

    /// Set TCP_NODELAY on connections. Default: `true`.
    pub fn tcp_nodelay(mut self, nodelay: bool) -> Self {
        self.tcp_nodelay = nodelay;
        self
    }

    /// Set the global maximum number of concurrent connections.
    ///
    /// Caps the total live connections in the pool across all hosts,
    /// including idle (cached) connections. New connection attempts wait
    /// when the pool is at capacity; existing connections must be evicted
    /// or closed before another can be created.
    ///
    /// Should be at least [`max_connections_per_host`](Self::max_connections_per_host)
    /// when both are set; the global limit applies on top of the per-host
    /// limit.
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
    pub fn proxy_config(mut self, config: crate::client::proxy::ProxyConfig) -> Self {
        self.proxy_config = Some(config);
        self
    }

    /// Mutable-reference variant of [`proxy_config`](Self::proxy_config).
    pub fn set_proxy_config(
        &mut self,
        config: Option<crate::client::proxy::ProxyConfig>,
    ) -> &mut Self {
        self.proxy_config = config;
        self
    }

    /// Set a listener for connection lifecycle events (created, reused, closed, failed).
    pub fn connection_event_listener(
        mut self,
        listener: Arc<dyn super::connection::ConnectionEventListener>,
    ) -> Self {
        self.connection_event_listener = Some(listener);
        self
    }

    /// Mutable-reference variant of [`connection_event_listener`](Self::connection_event_listener).
    pub fn set_connection_event_listener(
        &mut self,
        listener: Option<Arc<dyn super::connection::ConnectionEventListener>>,
    ) -> &mut Self {
        self.connection_event_listener = listener;
        self
    }

    /// Set the policy that governs checkout when the local partition has
    /// no idle connection and the pool is at capacity.
    ///
    /// Defaults to [`CrossPartitionPolicy::Never`](super::partition::CrossPartitionPolicy::Never).
    /// Has no observable effect with a single partition or when the pool
    /// stays under capacity.
    pub fn cross_partition_policy(
        mut self,
        policy: super::partition::CrossPartitionPolicy,
    ) -> Self {
        self.cross_partition_policy = policy;
        self
    }

    /// Mutable-reference variant of [`cross_partition_policy`](Self::cross_partition_policy).
    pub fn set_cross_partition_policy(
        &mut self,
        policy: super::partition::CrossPartitionPolicy,
    ) -> &mut Self {
        self.cross_partition_policy = policy;
        self
    }

    /// Set a custom DNS resolver.
    ///
    /// Connections established by this pool resolve hostnames through the
    /// given resolver. Defaults to the system resolver when unset.
    pub fn dns_resolver(
        mut self,
        resolver: impl aws_smithy_runtime_api::client::dns::ResolveDns + 'static,
    ) -> Self {
        use aws_smithy_runtime_api::shared::IntoShared;
        self.dns_resolver = Some(resolver.into_shared());
        self
    }

    /// Mutable-reference variant of [`dns_resolver`](Self::dns_resolver).
    pub fn set_dns_resolver(
        &mut self,
        resolver: Option<aws_smithy_runtime_api::client::dns::SharedDnsResolver>,
    ) -> &mut Self {
        self.dns_resolver = resolver;
        self
    }
}

impl Builder<TlsUnset> {
    /// Set the TLS implementation.
    pub fn tls_provider(self, provider: tls::Provider) -> Builder<TlsProviderSelected> {
        Builder {
            pool_idle_timeout: self.pool_idle_timeout,
            tcp_nodelay: self.tcp_nodelay,
            max_connections: self.max_connections,
            max_connections_per_host: self.max_connections_per_host,
            proxy_config: self.proxy_config,
            connection_event_listener: self.connection_event_listener,
            cross_partition_policy: self.cross_partition_policy,
            dns_resolver: self.dns_resolver,
            tls: TlsProviderSelected {
                provider,
                context: TlsContext::default(),
            },
        }
    }

    /// Build an HTTP client without TLS.
    #[doc(hidden)]
    pub fn build_http(mut self) -> super::SharedPool {
        let dns_resolver = self.dns_resolver.take();
        let config = PoolConfig {
            max_connections: self.max_connections,
            max_connections_per_host: self.max_connections_per_host,
            pool_idle_timeout: self.pool_idle_timeout,
            connection_event_listener: self.connection_event_listener.clone(),
        };
        let proxy_matcher = proxy_matcher_from(&self.proxy_config);
        let pool = match dns_resolver {
            Some(resolver) => {
                use crate::client::dns::HyperUtilResolver;
                let mut tcp = HyperHttpConnector::new_with_resolver(HyperUtilResolver { resolver });
                tcp.set_nodelay(self.tcp_nodelay);
                build_http_pool_with_proxy(tcp, &self.proxy_config, config)
            }
            None => {
                let mut tcp = HyperHttpConnector::new();
                tcp.set_nodelay(self.tcp_nodelay);
                build_http_pool_with_proxy(tcp, &self.proxy_config, config)
            }
        };
        super::SharedPool {
            inner: Arc::new(super::SharedPoolInner {
                pool: Arc::new(pool),
                proxy_matcher,
            }),
        }
    }

    /// Build an HTTP client from a raw TCP-level connector.
    ///
    /// The connector must be a `tower::Service<Uri>` producing an IO type
    /// that implements hyper's `Read`, `Write`, and `Connection` traits.
    /// The pool's Negotiate layer uses `Connection::connected().is_negotiated_h2()`
    /// to route connections to the H2 path.
    #[cfg(all(feature = "test-util", aws_sdk_unstable))]
    #[doc(hidden)]
    pub fn build_http_with_tcp_connector<C, IO>(self, connector: C) -> super::SharedPool
    where
        C: tower::Service<http_1x::Uri, Response = IO> + Clone + Send + Sync + 'static,
        C::Error: Into<aws_smithy_runtime_api::box_error::BoxError> + 'static,
        C::Future: Unpin + Send + 'static,
        IO: hyper::rt::Read
            + hyper::rt::Write
            + hyper_util::client::legacy::connect::Connection
            + Unpin
            + Send
            + 'static,
    {
        let config = super::PoolConfig {
            max_connections: self.max_connections,
            max_connections_per_host: self.max_connections_per_host,
            pool_idle_timeout: self.pool_idle_timeout,
            connection_event_listener: self.connection_event_listener.clone(),
        };
        let pool = super::build_pool(connector, config);
        super::SharedPool {
            inner: Arc::new(super::SharedPoolInner {
                pool: Arc::new(pool),
                proxy_matcher: None,
            }),
        }
    }
}

/// Build a no-TLS pool that honors `proxy_config`. Wraps the TCP connector
/// with an HTTP proxy connector when configured. Emits a warning if an
/// HTTPS proxy is set without a TLS provider; connections to such a
/// proxy will fail at handshake time.
fn build_http_pool_with_proxy<R>(
    tcp: HyperHttpConnector<R>,
    proxy_config: &Option<crate::client::proxy::ProxyConfig>,
    config: PoolConfig,
) -> ConnectionPool
where
    R: Clone + Send + Sync + 'static,
    R: tower::Service<hyper_util::client::legacy::connect::dns::Name>,
    R::Response: Iterator<Item = std::net::SocketAddr>,
    R::Future: Send,
    R::Error: Into<aws_smithy_runtime_api::box_error::BoxError>,
{
    use crate::client::proxy;

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
        super::build_pool(tcp, config)
    } else {
        let connector = crate::client::connect::HttpProxyConnector::new(tcp, proxy_config);
        super::build_pool(connector, config)
    }
}

impl Builder<TlsProviderSelected> {
    /// Set the TLS context (custom trust store, etc.).
    pub fn tls_context(mut self, context: TlsContext) -> Self {
        self.tls.context = context;
        self
    }

    /// Build an HTTPS client with the selected TLS provider.
    pub fn build_https(mut self) -> super::SharedPool {
        let dns_resolver = self.dns_resolver.take();
        match dns_resolver {
            Some(resolver) => {
                use crate::client::dns::HyperUtilResolver;
                let mut tcp = HyperHttpConnector::new_with_resolver(HyperUtilResolver { resolver });
                tcp.set_nodelay(self.tcp_nodelay);
                tcp.enforce_http(false);
                self.build_from_tcp(tcp)
            }
            None => {
                let mut tcp = HyperHttpConnector::new();
                tcp.set_nodelay(self.tcp_nodelay);
                tcp.enforce_http(false);
                self.build_from_tcp(tcp)
            }
        }
    }

    fn build_from_tcp<R>(self, tcp: HyperHttpConnector<R>) -> super::SharedPool
    where
        R: Clone + Send + Sync + 'static,
        R: tower::Service<hyper_util::client::legacy::connect::dns::Name>,
        R::Response: Iterator<Item = std::net::SocketAddr>,
        R::Future: Send,
        R::Error: Into<aws_smithy_runtime_api::box_error::BoxError>,
    {
        use crate::client::proxy;

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
                let pool = super::build_pool(connector, config);
                super::SharedPool {
                    inner: Arc::new(super::SharedPoolInner {
                        pool: Arc::new(pool),
                        proxy_matcher,
                    }),
                }
            }
            #[cfg(feature = "s2n-tls")]
            tls::Provider::S2nTls => {
                let connector = tls::s2n_tls_provider::build_connector::wrap_connector(
                    tcp,
                    &self.tls.context,
                    proxy_config,
                );
                let pool = super::build_pool(connector, config);
                super::SharedPool {
                    inner: Arc::new(super::SharedPoolInner {
                        pool: Arc::new(pool),
                        proxy_matcher,
                    }),
                }
            }
            // Provider is #[non_exhaustive]; this arm is unreachable when any
            // TLS feature is enabled (which is required to construct a Provider).
            #[allow(unreachable_patterns)]
            _ => unreachable!("a TLS feature must be enabled to use build_https()"),
        }
    }
}

/// Build the proxy URL matcher from a `ProxyConfig`, returning `None` when
/// no proxy is configured. Wrapped in `Arc` so each `PooledConnector`
/// constructed by `http_connector` can share the (immutable) matcher
/// without per-call allocation.
pub(super) fn proxy_matcher_from(
    proxy_config: &Option<crate::client::proxy::ProxyConfig>,
) -> Option<Arc<hyper_util::client::proxy::matcher::Matcher>> {
    proxy_config
        .as_ref()
        .map(|c| Arc::new(c.clone().into_hyper_util_matcher()))
}

/// Returns the port if it is not the default for the scheme.
pub(super) fn get_non_default_port(uri: &http_1x::Uri) -> Option<http_1x::uri::Port<&str>> {
    match (uri.port().map(|p| p.as_u16()), uri.scheme()) {
        (Some(443), Some(s)) if *s == http_1x::uri::Scheme::HTTPS => None,
        (Some(80), Some(s)) if *s == http_1x::uri::Scheme::HTTP => None,
        _ => uri.port(),
    }
}
