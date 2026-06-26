/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Connection pool builder.
//!
//! Entry point: [`SharedPool::builder`](super::SharedPool::builder).

use std::sync::Arc;
use std::time::Duration;

use aws_smithy_runtime_api::client::dns::{ResolveDns, SharedDnsResolver};
use aws_smithy_runtime_api::shared::IntoShared;
use hyper_util::client::legacy::connect::dns::Name as DnsName;
use hyper_util::client::legacy::connect::HttpConnector as HyperHttpConnector;
use hyper_util::client::proxy::matcher::Matcher as ProxyMatcher;

use super::connection::ConnectionEventListener;
use super::partition::{CrossPartitionPolicy, Partition};
use super::{BoxError, ConnectionPool, PoolConfig};
use crate::client::dns::HyperUtilResolver;
use crate::client::proxy::ProxyConfig;
use crate::client::tls;
use crate::client::{TlsProviderSelected, TlsUnset};
use crate::tls::TlsContext;

/// Default idle-connection eviction timeout, applied when the caller does
/// not configure one.
const DEFAULT_POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// Builder for a [`SharedPool`].
///
/// Configures pool-wide settings: TLS, DNS, connection limits, idle
/// eviction, proxy, partition topology, and connection event listening.
///
/// Type-state ensures TLS is configured before [`build_https`] is
/// callable; calling [`tls_provider`] transitions the builder into the
/// state where [`build_https`] is available.
///
/// [`build_https`]: Builder::build_https
/// [`tls_provider`]: Builder::tls_provider
/// [`SharedPool`]: super::SharedPool
#[derive(Clone)]
pub struct Builder<Tls = TlsUnset> {
    pool_idle_timeout: Option<Option<Duration>>,
    tcp_nodelay: bool,
    tcp_keepalive: Option<Option<Duration>>,
    max_connections: Option<usize>,
    max_connections_per_host: Option<usize>,
    proxy_config: Option<ProxyConfig>,
    connection_event_listener: Option<Arc<dyn ConnectionEventListener>>,
    cross_partition_policy: CrossPartitionPolicy,
    dns_resolver: Option<SharedDnsResolver>,
    partitions: Vec<Partition>,
    tls: Tls,
}

impl<Tls: std::fmt::Debug> std::fmt::Debug for Builder<Tls> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Builder")
            .field("pool_idle_timeout", &self.pool_idle_timeout)
            .field("tcp_nodelay", &self.tcp_nodelay)
            .field("tcp_keepalive", &self.tcp_keepalive)
            .field("max_connections", &self.max_connections)
            .field("max_connections_per_host", &self.max_connections_per_host)
            .field("proxy_config", &self.proxy_config)
            .field(
                "connection_event_listener",
                &self.connection_event_listener.as_ref().map(|_| ".."),
            )
            .field("cross_partition_policy", &self.cross_partition_policy)
            .field("dns_resolver", &self.dns_resolver.as_ref().map(|_| ".."))
            .field("partitions", &self.partitions.len())
            .finish()
    }
}

impl Default for Builder<TlsUnset> {
    fn default() -> Self {
        Self {
            pool_idle_timeout: None,
            tcp_nodelay: true,
            tcp_keepalive: None,
            max_connections: None,
            max_connections_per_host: None,
            proxy_config: None,
            connection_event_listener: None,
            cross_partition_policy: CrossPartitionPolicy::default(),
            dns_resolver: None,
            partitions: Vec::new(),
            tls: TlsUnset {},
        }
    }
}

// Methods available in any TLS state.
impl<Tls> Builder<Tls> {
    /// Set the pool idle timeout.
    ///
    /// Connections idle longer than this duration are evicted from the
    /// pool. Set below the server's idle timeout to avoid dispatching on a
    /// connection the server has already closed.
    ///
    /// Unset, the pool uses a default of 60 seconds. Pass `Some(duration)`
    /// to override, or `None` to disable idle eviction entirely.
    pub fn pool_idle_timeout<D>(mut self, timeout: D) -> Self
    where
        D: Into<Option<Duration>>,
    {
        self.pool_idle_timeout = Some(timeout.into());
        self
    }

    /// This is the mutable version of [`pool_idle_timeout`](Self::pool_idle_timeout).
    ///
    /// The outer `None` selects the default; `Some(None)` disables idle
    /// eviction; `Some(Some(d))` sets the timeout to `d`.
    pub fn set_pool_idle_timeout(&mut self, timeout: Option<Option<Duration>>) -> &mut Self {
        self.pool_idle_timeout = timeout;
        self
    }

    /// Set TCP_NODELAY on connections. Default: `true`.
    pub fn tcp_nodelay(mut self, nodelay: bool) -> Self {
        self.tcp_nodelay = nodelay;
        self
    }

    /// This is the mutable version of [`tcp_nodelay`](Self::tcp_nodelay).
    pub fn set_tcp_nodelay(&mut self, nodelay: bool) -> &mut Self {
        self.tcp_nodelay = nodelay;
        self
    }

    /// Set the TCP keepalive idle time.
    ///
    /// Enables `SO_KEEPALIVE` with the given idle time before the first
    /// probe. Keepalive detects dead peers faster than idle eviction
    /// alone, notably for long-lived H2 connections.
    ///
    /// Keepalive is disabled by default. Pass `Some(duration)` to enable
    /// it with that idle time; `None` leaves it disabled.
    pub fn tcp_keepalive<D>(mut self, time: D) -> Self
    where
        D: Into<Option<Duration>>,
    {
        self.tcp_keepalive = Some(time.into());
        self
    }

    /// This is the mutable version of [`tcp_keepalive`](Self::tcp_keepalive).
    ///
    /// The outer `None` selects the default; `Some(None)` disables
    /// keepalive; `Some(Some(d))` sets the idle time to `d`.
    pub fn set_tcp_keepalive(&mut self, time: Option<Option<Duration>>) -> &mut Self {
        self.tcp_keepalive = time;
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
    /// limit. A global limit below the per-host limit clamps every host to
    /// the global value (the per-host limit can never be reached) and logs a
    /// warning at build time.
    pub fn max_connections(mut self, n: usize) -> Self {
        self.max_connections = Some(n);
        self
    }

    /// This is the mutable version of [`max_connections`](Self::max_connections).
    ///
    /// `None` leaves the global limit unset (unbounded).
    pub fn set_max_connections(&mut self, n: Option<usize>) -> &mut Self {
        self.max_connections = n;
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

    /// This is the mutable version of
    /// [`max_connections_per_host`](Self::max_connections_per_host).
    ///
    /// `None` leaves the per-host limit unset (unbounded).
    pub fn set_max_connections_per_host(&mut self, n: Option<usize>) -> &mut Self {
        self.max_connections_per_host = n;
        self
    }

    /// Route connections through an HTTP/HTTPS/SOCKS proxy.
    ///
    /// Per-host proxy resolution is stable for the lifetime of the
    /// pool: all connections to a given authority follow the same
    /// proxy decision. HTTPS through an HTTP proxy uses `CONNECT`
    /// tunneling.
    ///
    /// Pass [`ProxyConfig::from_env`] to read configuration from the
    /// `HTTP_PROXY`, `HTTPS_PROXY`, and `NO_PROXY` environment variables.
    pub fn proxy_config(mut self, config: ProxyConfig) -> Self {
        self.proxy_config = Some(config);
        self
    }

    /// This is the mutable version of [`proxy_config`](Self::proxy_config).
    pub fn set_proxy_config(&mut self, config: Option<ProxyConfig>) -> &mut Self {
        self.proxy_config = config;
        self
    }

    /// Set a listener for connection lifecycle events (created, reused, closed, failed).
    pub fn connection_event_listener(mut self, listener: Arc<dyn ConnectionEventListener>) -> Self {
        self.connection_event_listener = Some(listener);
        self
    }

    /// This is the mutable version of [`connection_event_listener`](Self::connection_event_listener).
    pub fn set_connection_event_listener(
        &mut self,
        listener: Option<Arc<dyn ConnectionEventListener>>,
    ) -> &mut Self {
        self.connection_event_listener = listener;
        self
    }

    /// Set the policy that governs checkout when the local partition has
    /// no idle connection and the pool is at capacity.
    ///
    /// Defaults to [`CrossPartitionPolicy::Never`]. Has no observable
    /// effect with a single partition or when the pool stays under
    /// capacity.
    pub fn cross_partition_policy(mut self, policy: CrossPartitionPolicy) -> Self {
        self.cross_partition_policy = policy;
        self
    }

    /// This is the mutable version of [`cross_partition_policy`](Self::cross_partition_policy).
    pub fn set_cross_partition_policy(&mut self, policy: CrossPartitionPolicy) -> &mut Self {
        self.cross_partition_policy = policy;
        self
    }

    /// Set a custom DNS resolver.
    ///
    /// Connections established by this pool resolve hostnames through the
    /// given resolver. Defaults to the system resolver when unset.
    pub fn dns_resolver(mut self, resolver: impl ResolveDns + 'static) -> Self {
        self.dns_resolver = Some(resolver.into_shared());
        self
    }

    /// This is the mutable version of [`dns_resolver`](Self::dns_resolver).
    pub fn set_dns_resolver(&mut self, resolver: Option<SharedDnsResolver>) -> &mut Self {
        self.dns_resolver = resolver;
        self
    }

    /// Declare the pool's partition topology. Each [`Partition`] carries a
    /// driver-spawner runtime and optional NIC binding. When omitted, the
    /// pool uses a single anonymous partition on the current tokio runtime.
    pub fn partitions(mut self, partitions: impl IntoIterator<Item = Partition>) -> Self {
        self.partitions = partitions.into_iter().collect();
        self
    }

    /// This is the mutable version of [`partitions`](Self::partitions).
    ///
    /// `None` clears any declared topology; the pool then uses a single
    /// anonymous partition on the current tokio runtime.
    pub fn set_partitions(&mut self, partitions: Option<Vec<Partition>>) -> &mut Self {
        self.partitions = partitions.unwrap_or_default();
        self
    }
}

impl Builder<TlsUnset> {
    /// Set the TLS implementation.
    pub fn tls_provider(self, provider: tls::Provider) -> Builder<TlsProviderSelected> {
        Builder {
            pool_idle_timeout: self.pool_idle_timeout,
            tcp_nodelay: self.tcp_nodelay,
            tcp_keepalive: self.tcp_keepalive,
            max_connections: self.max_connections,
            max_connections_per_host: self.max_connections_per_host,
            proxy_config: self.proxy_config,
            connection_event_listener: self.connection_event_listener,
            cross_partition_policy: self.cross_partition_policy,
            dns_resolver: self.dns_resolver,
            partitions: self.partitions,
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
            pool_idle_timeout: resolve_pool_idle_timeout(self.pool_idle_timeout),
            connection_event_listener: self.connection_event_listener.clone(),
        };
        let keepalive = resolve_tcp_keepalive(self.tcp_keepalive);
        let proxy_matcher = proxy_matcher_from(&self.proxy_config);
        let partitions = std::mem::take(&mut self.partitions);
        let policy = self.cross_partition_policy;
        let pool = match dns_resolver {
            Some(resolver) => {
                let mut tcp = HyperHttpConnector::new_with_resolver(HyperUtilResolver { resolver });
                tcp.set_nodelay(self.tcp_nodelay);
                tcp.set_keepalive(keepalive);
                build_http_pool_with_proxy(tcp, &self.proxy_config, config, partitions, policy)
            }
            None => {
                let mut tcp = HyperHttpConnector::new();
                tcp.set_nodelay(self.tcp_nodelay);
                tcp.set_keepalive(keepalive);
                build_http_pool_with_proxy(tcp, &self.proxy_config, config, partitions, policy)
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
    ///
    /// NIC binding is not applied to custom TCP connectors — the custom
    /// connector owns its own socket configuration.
    #[cfg(all(feature = "test-util", aws_sdk_unstable))]
    #[doc(hidden)]
    pub fn build_http_with_tcp_connector<C, IO>(self, connector: C) -> super::SharedPool
    where
        C: tower::Service<http_1x::Uri, Response = IO> + Clone + Send + Sync + 'static,
        C::Error: Into<BoxError> + 'static,
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
            pool_idle_timeout: resolve_pool_idle_timeout(self.pool_idle_timeout),
            connection_event_listener: self.connection_event_listener.clone(),
        };
        let policy = self.cross_partition_policy;
        let connector_factory = move |_partition: &Partition| connector.clone();
        let pool = super::build_pool(connector_factory, config, self.partitions, policy);
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
    proxy_config: &Option<ProxyConfig>,
    config: PoolConfig,
    partitions: Vec<Partition>,
    policy: CrossPartitionPolicy,
) -> ConnectionPool
where
    R: Clone + Send + Sync + 'static,
    R: tower::Service<DnsName>,
    R::Response: Iterator<Item = std::net::SocketAddr>,
    R::Future: Send,
    R::Error: Into<BoxError>,
{
    let proxy_config = proxy_config.clone().unwrap_or_else(ProxyConfig::disabled);

    if proxy_config.requires_tls() {
        tracing::warn!(
            "HTTPS proxy configured but no TLS provider set. \
             Connections to HTTPS proxy servers will fail. \
             Consider configuring a TLS provider to enable TLS support."
        );
    }

    if proxy_config.is_disabled() {
        let connector_factory =
            move |partition: &Partition| bind_interface(&tcp, partition.nic.as_deref());
        super::build_pool(connector_factory, config, partitions, policy)
    } else {
        let connector_factory = move |partition: &Partition| {
            crate::client::connect::HttpProxyConnector::new(
                bind_interface(&tcp, partition.nic.as_deref()),
                proxy_config.clone(),
            )
        };
        super::build_pool(connector_factory, config, partitions, policy)
    }
}

impl Builder<TlsProviderSelected> {
    /// Set the TLS context (custom trust store, etc.).
    pub fn tls_context(mut self, context: TlsContext) -> Self {
        self.tls.context = context;
        self
    }

    /// This is the mutable version of [`tls_context`](Self::tls_context).
    pub fn set_tls_context(&mut self, context: TlsContext) -> &mut Self {
        self.tls.context = context;
        self
    }

    /// Build an HTTPS client with the selected TLS provider.
    pub fn build_https(mut self) -> super::SharedPool {
        let dns_resolver = self.dns_resolver.take();
        let keepalive = resolve_tcp_keepalive(self.tcp_keepalive);
        match dns_resolver {
            Some(resolver) => {
                let mut tcp = HyperHttpConnector::new_with_resolver(HyperUtilResolver { resolver });
                tcp.set_nodelay(self.tcp_nodelay);
                tcp.set_keepalive(keepalive);
                tcp.enforce_http(false);
                self.build_from_tcp(tcp)
            }
            None => {
                let mut tcp = HyperHttpConnector::new();
                tcp.set_nodelay(self.tcp_nodelay);
                tcp.set_keepalive(keepalive);
                tcp.enforce_http(false);
                self.build_from_tcp(tcp)
            }
        }
    }

    fn build_from_tcp<R>(self, tcp: HyperHttpConnector<R>) -> super::SharedPool
    where
        R: Clone + Send + Sync + 'static,
        R: tower::Service<DnsName>,
        R::Response: Iterator<Item = std::net::SocketAddr>,
        R::Future: Send,
        R::Error: Into<BoxError>,
    {
        let config = PoolConfig {
            max_connections: self.max_connections,
            max_connections_per_host: self.max_connections_per_host,
            pool_idle_timeout: resolve_pool_idle_timeout(self.pool_idle_timeout),
            connection_event_listener: self.connection_event_listener.clone(),
        };

        let proxy_config = self
            .proxy_config
            .clone()
            .unwrap_or_else(ProxyConfig::disabled);
        let proxy_matcher = proxy_matcher_from(&self.proxy_config);
        let partitions = self.partitions;
        let policy = self.cross_partition_policy;

        match &self.tls.provider {
            #[cfg(any(
                feature = "rustls-aws-lc",
                feature = "rustls-aws-lc-fips",
                feature = "rustls-ring"
            ))]
            tls::Provider::Rustls(crypto_mode) => {
                let crypto_mode = crypto_mode.clone();
                let tls_context = self.tls.context.clone();
                let connector_factory = move |partition: &Partition| {
                    tls::rustls_provider::build_connector::wrap_connector(
                        bind_interface(&tcp, partition.nic.as_deref()),
                        crypto_mode.clone(),
                        &tls_context,
                        proxy_config.clone(),
                    )
                };
                let pool = super::build_pool(connector_factory, config, partitions, policy);
                super::SharedPool {
                    inner: Arc::new(super::SharedPoolInner {
                        pool: Arc::new(pool),
                        proxy_matcher,
                    }),
                }
            }
            #[cfg(feature = "s2n-tls")]
            tls::Provider::S2nTls => {
                let tls_context = self.tls.context.clone();
                let connector_factory = move |partition: &Partition| {
                    tls::s2n_tls_provider::build_connector::wrap_connector(
                        bind_interface(&tcp, partition.nic.as_deref()),
                        &tls_context,
                        proxy_config.clone(),
                    )
                };
                let pool = super::build_pool(connector_factory, config, partitions, policy);
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

/// Resolve the configured pool idle timeout. Outer `None` applies the
/// default; `Some(None)` disables eviction; `Some(Some(d))` uses `d`.
fn resolve_pool_idle_timeout(configured: Option<Option<Duration>>) -> Option<Duration> {
    match configured {
        None => Some(DEFAULT_POOL_IDLE_TIMEOUT),
        Some(inner) => inner,
    }
}

/// Resolve the configured TCP keepalive idle time. Keepalive is disabled
/// by default, so the outer `None` resolves to off. `Some(None)` is also
/// off; `Some(Some(d))` enables keepalive with idle time `d`.
fn resolve_tcp_keepalive(configured: Option<Option<Duration>>) -> Option<Duration> {
    configured.flatten()
}

/// Bind a clone of the base TCP connector to a network interface.
///
/// The single place per-partition NIC binding happens: each partition's
/// connector factory clones the shared base connector and routes it
/// through here with that partition's `nic`. `None` (the default,
/// no-interface case) returns an unbound clone. `set_interface` is only
/// available on Linux-like targets; elsewhere the `nic` is accepted and
/// ignored, matching the v1 client.
fn bind_interface<R>(base: &HyperHttpConnector<R>, nic: Option<&str>) -> HyperHttpConnector<R>
where
    R: Clone,
{
    #[allow(unused_mut)]
    let mut tcp = base.clone();
    #[cfg(any(target_os = "android", target_os = "fuchsia", target_os = "linux"))]
    if let Some(interface) = nic {
        tcp.set_interface(interface);
    }
    let _ = nic;
    tcp
}

/// Build the proxy URL matcher from a `ProxyConfig`, returning `None` when
/// no proxy is configured. Wrapped in `Arc` for shared ownership: the pool
/// stores it once and each `PooledConnector` clones the handle.
pub(super) fn proxy_matcher_from(proxy_config: &Option<ProxyConfig>) -> Option<Arc<ProxyMatcher>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::pool::PartitionId;

    #[test]
    fn builder_defaults() {
        let b = Builder::default();
        // Outer None = "not configured"; the default is applied at build time.
        assert_eq!(b.pool_idle_timeout, None);
        assert_eq!(b.tcp_keepalive, None);
        assert!(b.tcp_nodelay, "tcp_nodelay defaults to true");
        assert_eq!(b.max_connections, None);
        assert_eq!(b.max_connections_per_host, None);
        assert!(b.proxy_config.is_none());
        assert!(b.connection_event_listener.is_none());
        assert_eq!(b.cross_partition_policy, CrossPartitionPolicy::Never);
        assert!(b.dns_resolver.is_none());
        assert!(b.partitions.is_empty());
    }

    #[test]
    fn idle_timeout_resolution() {
        // Unconfigured → default applied.
        assert_eq!(
            resolve_pool_idle_timeout(None),
            Some(DEFAULT_POOL_IDLE_TIMEOUT)
        );
        // Some(None) → explicitly disabled.
        assert_eq!(resolve_pool_idle_timeout(Some(None)), None);
        // Some(Some(d)) → overridden.
        let d = Duration::from_secs(5);
        assert_eq!(resolve_pool_idle_timeout(Some(Some(d))), Some(d));
    }

    #[test]
    fn keepalive_resolution() {
        // Unset → off (keepalive is disabled by default).
        assert_eq!(resolve_tcp_keepalive(None), None);
        assert_eq!(resolve_tcp_keepalive(Some(None)), None);
        let d = Duration::from_secs(45);
        assert_eq!(resolve_tcp_keepalive(Some(Some(d))), Some(d));
    }

    #[test]
    fn idle_timeout_setters_set_three_states() {
        // chaining setter: Duration → Some(Some(d))
        let b = Builder::default().pool_idle_timeout(Duration::from_secs(7));
        assert_eq!(b.pool_idle_timeout, Some(Some(Duration::from_secs(7))));
        // chaining setter: None → Some(None) (disable)
        let b = Builder::default().pool_idle_timeout(None);
        assert_eq!(b.pool_idle_timeout, Some(None));
        // mutable setter passes through verbatim
        let mut b = Builder::default();
        b.set_pool_idle_timeout(Some(None));
        assert_eq!(b.pool_idle_timeout, Some(None));
    }

    #[test]
    fn keepalive_setters_set_three_states() {
        let b = Builder::default().tcp_keepalive(Duration::from_secs(15));
        assert_eq!(b.tcp_keepalive, Some(Some(Duration::from_secs(15))));
        let b = Builder::default().tcp_keepalive(None);
        assert_eq!(b.tcp_keepalive, Some(None));
        let mut b = Builder::default();
        b.set_tcp_keepalive(Some(None));
        assert_eq!(b.tcp_keepalive, Some(None));
    }

    /// `tls_provider` transitions the type-state while preserving every
    /// configured field. Guards the hand-written field-by-field move in
    /// `tls_provider` against a dropped field on a future edit.
    #[test]
    fn tls_provider_preserves_all_config() {
        let b = Builder::default()
            .pool_idle_timeout(Duration::from_secs(30))
            .tcp_nodelay(false)
            .tcp_keepalive(Duration::from_secs(45))
            .max_connections(100)
            .max_connections_per_host(10)
            .cross_partition_policy(CrossPartitionPolicy::PreferLocal)
            .partitions([Partition::new(
                PartitionId::from_index(0),
                crate::client::pool::TokioDriverSpawner::from_handle(
                    // a handle is only needed to construct the spawner; no
                    // runtime work happens here.
                    tokio::runtime::Builder::new_current_thread()
                        .build()
                        .unwrap()
                        .handle()
                        .clone(),
                ),
            )]);

        let provider = tls::Provider::Rustls(tls::rustls_provider::CryptoMode::AwsLc);
        let b = b.tls_provider(provider);

        assert_eq!(b.pool_idle_timeout, Some(Some(Duration::from_secs(30))));
        assert!(!b.tcp_nodelay);
        assert_eq!(b.tcp_keepalive, Some(Some(Duration::from_secs(45))));
        assert_eq!(b.max_connections, Some(100));
        assert_eq!(b.max_connections_per_host, Some(10));
        assert_eq!(b.cross_partition_policy, CrossPartitionPolicy::PreferLocal);
        assert_eq!(b.partitions.len(), 1);
    }

    #[test]
    fn non_default_port_elided_per_scheme() {
        let cases = [
            ("https://example.com/", None), // 443 elided
            ("http://example.com/", None),  // 80 elided
            ("https://example.com:8443/", Some(8443)),
            ("http://example.com:8080/", Some(8080)),
            ("https://example.com:80/", Some(80)), // 80 is non-default for https
            ("http://example.com:443/", Some(443)), // 443 is non-default for http
        ];
        for (uri, expected) in cases {
            let uri: http_1x::Uri = uri.parse().unwrap();
            let port = get_non_default_port(&uri).map(|p| p.as_u16());
            assert_eq!(port, expected, "uri = {uri}");
        }
    }
}
