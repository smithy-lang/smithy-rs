/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Construction and validation for [`ConnectionPool`].

use super::handshake::{self, TransportFactory};
use super::maintenance::MaintenanceConfig;
use super::registry::{PartitionRegistry, PartitionRegistryError};
use super::{ConnectionPool, ConnectionReuseScope, Partition, PoolConfig, PoolInner};
use crate::client::TlsUnset;
use aws_smithy_async::rt::sleep::{default_async_sleep, AsyncSleep, SharedAsyncSleep};
use aws_smithy_async::time::{SharedTimeSource, TimeSource};
#[cfg(any(
    all(test, feature = "rt-tokio"),
    all(feature = "test-util", aws_sdk_unstable)
))]
use aws_smithy_runtime_api::box_error::BoxError;
#[cfg(any(
    all(test, feature = "rt-tokio"),
    all(feature = "test-util", aws_sdk_unstable)
))]
use http_1x::Uri;
use hyper_util::client::legacy::connect::dns::GaiResolver;
use hyper_util::client::legacy::connect::HttpConnector;
use std::error::Error;
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;
#[cfg(any(
    all(test, feature = "rt-tokio"),
    all(feature = "test-util", aws_sdk_unstable)
))]
use tower::Service;

const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

/// Builds a partition-aware connection pool.
#[derive(Clone)]
pub struct Builder<Tls = TlsUnset> {
    idle_timeout: Option<Option<Duration>>,
    time_source: Option<SharedTimeSource>,
    sleep_impl: Option<SharedAsyncSleep>,
    tcp_nodelay: bool,
    tcp_keepalive: Option<Option<Duration>>,
    max_connections_per_host: Option<usize>,
    reuse_scope: ConnectionReuseScope,
    partitions: Option<Vec<Partition>>,
    tls: Tls,
}

impl Default for Builder<TlsUnset> {
    fn default() -> Self {
        Self {
            idle_timeout: None,
            time_source: None,
            sleep_impl: None,
            tcp_nodelay: true,
            tcp_keepalive: None,
            max_connections_per_host: None,
            reuse_scope: ConnectionReuseScope::default(),
            partitions: None,
            tls: TlsUnset {},
        }
    }
}

impl<Tls: fmt::Debug> fmt::Debug for Builder<Tls> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Builder")
            .field("idle_timeout", &self.idle_timeout)
            .field("time_source", &self.time_source)
            .field("sleep_impl", &self.sleep_impl)
            .field("tcp_nodelay", &self.tcp_nodelay)
            .field("tcp_keepalive", &self.tcp_keepalive)
            .field("max_connections_per_host", &self.max_connections_per_host)
            .field("reuse_scope", &self.reuse_scope)
            .field("partitions", &self.partitions.as_ref().map(Vec::len))
            .field("tls", &self.tls)
            .finish()
    }
}

impl<Tls> Builder<Tls> {
    /// Sets how long a reusable idle connection remains in the pool.
    ///
    /// Passing `None` disables idle expiration. When unset, the pool uses a
    /// 90-second timeout.
    pub fn idle_timeout(mut self, timeout: impl Into<Option<Duration>>) -> Self {
        self.idle_timeout = Some(timeout.into());
        self
    }

    /// Mutably sets the idle timeout.
    ///
    /// The outer `None` restores the default; `Some(None)` disables expiry.
    pub fn set_idle_timeout(&mut self, timeout: Option<Option<Duration>>) -> &mut Self {
        self.idle_timeout = timeout;
        self
    }

    /// Sets the clock used by pool maintenance.
    pub fn time_source(mut self, source: impl TimeSource + 'static) -> Self {
        self.time_source = Some(SharedTimeSource::new(source));
        self
    }

    /// Mutably sets the clock used by pool maintenance.
    pub fn set_time_source(&mut self, source: Option<SharedTimeSource>) -> &mut Self {
        self.time_source = source;
        self
    }

    /// Sets the sleep implementation used by pool maintenance.
    pub fn sleep_impl(mut self, sleep: impl AsyncSleep + 'static) -> Self {
        self.sleep_impl = Some(SharedAsyncSleep::new(sleep));
        self
    }

    /// Mutably sets the sleep implementation used by pool maintenance.
    pub fn set_sleep_impl(&mut self, sleep: Option<SharedAsyncSleep>) -> &mut Self {
        self.sleep_impl = sleep;
        self
    }

    /// Configures `TCP_NODELAY` for newly created sockets.
    pub fn tcp_nodelay(mut self, enabled: bool) -> Self {
        self.tcp_nodelay = enabled;
        self
    }

    /// Mutably configures `TCP_NODELAY`.
    pub fn set_tcp_nodelay(&mut self, enabled: bool) -> &mut Self {
        self.tcp_nodelay = enabled;
        self
    }

    /// Configures TCP keepalive for newly created sockets.
    pub fn tcp_keepalive(mut self, time: impl Into<Option<Duration>>) -> Self {
        self.tcp_keepalive = Some(time.into());
        self
    }

    /// Mutably configures TCP keepalive.
    pub fn set_tcp_keepalive(&mut self, time: Option<Option<Duration>>) -> &mut Self {
        self.tcp_keepalive = time;
        self
    }

    /// Bounds live connections for one canonical origin across the pool.
    pub fn max_connections_per_host(mut self, limit: usize) -> Self {
        self.max_connections_per_host = Some(limit);
        self
    }

    /// Mutably sets the per-origin connection bound.
    pub fn set_max_connections_per_host(&mut self, limit: Option<usize>) -> &mut Self {
        self.max_connections_per_host = limit;
        self
    }

    /// Selects which partitions may reuse each other's connections.
    pub fn connection_reuse_scope(mut self, scope: ConnectionReuseScope) -> Self {
        self.reuse_scope = scope;
        self
    }

    /// Mutably selects the connection reuse scope.
    pub fn set_connection_reuse_scope(&mut self, scope: ConnectionReuseScope) -> &mut Self {
        self.reuse_scope = scope;
        self
    }

    /// Declares the complete explicit partition set.
    pub fn partitions(mut self, partitions: impl IntoIterator<Item = Partition>) -> Self {
        self.partitions = Some(partitions.into_iter().collect());
        self
    }

    /// Mutably declares or clears the explicit partition set.
    pub fn set_partitions(&mut self, partitions: Option<Vec<Partition>>) -> &mut Self {
        self.partitions = partitions;
        self
    }
}

impl Builder<TlsUnset> {
    /// Builds a pool for cleartext HTTP connections.
    #[doc(hidden)]
    pub fn build_http(self) -> Result<ConnectionPool, BuildError> {
        let mut connector = HttpConnector::new_with_resolver(GaiResolver::new());
        connector.set_nodelay(self.tcp_nodelay);
        connector.set_keepalive(self.tcp_keepalive.flatten());
        let transport = handshake::transport_factory(move |interface| {
            let mut connector = connector.clone();
            set_interface(&mut connector, interface);
            connector
        });
        self.finish(transport)
    }

    /// Builds a pool around a test transport connector.
    #[cfg(all(feature = "test-util", aws_sdk_unstable))]
    #[doc(hidden)]
    pub fn build_http_with_tcp_connector<C, IO>(
        self,
        connector: C,
    ) -> Result<ConnectionPool, BuildError>
    where
        C: Service<Uri, Response = IO> + Clone + Send + Sync + 'static,
        C::Error: Into<BoxError>,
        C::Future: Send + 'static,
        IO: crate::client::connect::AsyncConn,
    {
        self.finish(handshake::transport_factory(move |_| connector.clone()))
    }

    #[cfg(all(test, feature = "rt-tokio"))]
    pub(super) fn build_with_connector<C, IO>(
        self,
        connector: C,
    ) -> Result<ConnectionPool, BuildError>
    where
        C: Service<Uri, Response = IO> + Clone + Send + Sync + 'static,
        C::Error: Into<BoxError>,
        C::Future: Send + 'static,
        IO: crate::client::connect::AsyncConn,
    {
        self.finish(handshake::transport_factory(move |_| connector.clone()))
    }

    fn finish(self, transport: Arc<dyn TransportFactory>) -> Result<ConnectionPool, BuildError> {
        let max_connections_per_host = match self.max_connections_per_host {
            Some(0) => {
                return Err(BuildError::new(
                    "max_connections_per_host must be greater than zero",
                ))
            }
            Some(limit) => NonZeroUsize::new(limit),
            None => None,
        };
        let idle_timeout = self.idle_timeout.unwrap_or(Some(DEFAULT_IDLE_TIMEOUT));
        let time_source = self.time_source.unwrap_or_default();
        let sleep_impl = self.sleep_impl.or_else(default_async_sleep);
        if idle_timeout.is_some() && sleep_impl.is_none() {
            return Err(BuildError::new(
                "idle_timeout requires an async sleep implementation",
            ));
        }
        let maintenance = MaintenanceConfig {
            idle_timeout,
            time_source: time_source.clone(),
            sleep: sleep_impl.clone(),
        };
        let registry = match self.partitions {
            None => PartitionRegistry::anonymous_with_maintenance(
                self.reuse_scope,
                max_connections_per_host,
                maintenance.clone(),
            ),
            Some(partitions) => PartitionRegistry::explicit_with_maintenance(
                partitions,
                self.reuse_scope,
                max_connections_per_host,
                maintenance,
            )
            .map_err(BuildError::from)?,
        };
        let config = PoolConfig {
            idle_timeout,
            max_connections_per_host,
            reuse_scope: self.reuse_scope,
        };
        Ok(ConnectionPool {
            inner: Arc::new(PoolInner {
                config,
                registry,
                transport,
                next_connection_id: AtomicU64::new(0),
            }),
        })
    }
}

/// Error returned when a pool configuration is internally inconsistent.
#[derive(Debug)]
pub struct BuildError {
    message: String,
}

impl BuildError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl Error for BuildError {}

impl From<PartitionRegistryError> for BuildError {
    fn from(error: PartitionRegistryError) -> Self {
        Self::new(error.to_string())
    }
}

#[cfg(any(
    target_os = "android",
    target_os = "fuchsia",
    target_os = "illumos",
    target_os = "ios",
    target_os = "linux",
    target_os = "macos",
    target_os = "solaris",
    target_os = "tvos",
    target_os = "visionos",
    target_os = "watchos",
))]
fn set_interface(connector: &mut HttpConnector<GaiResolver>, interface: Option<&str>) {
    if let Some(interface) = interface {
        connector.set_interface(interface);
    }
}

#[cfg(not(any(
    target_os = "android",
    target_os = "fuchsia",
    target_os = "illumos",
    target_os = "ios",
    target_os = "linux",
    target_os = "macos",
    target_os = "solaris",
    target_os = "tvos",
    target_os = "visionos",
    target_os = "watchos",
)))]
fn set_interface(_connector: &mut HttpConnector<GaiResolver>, _interface: Option<&str>) {}

#[cfg(all(test, not(smithy_http_client_loom)))]
mod tests {
    use super::*;
    use crate::client::pool::{DriverSpawner, PartitionId};
    use std::future::Future;
    use std::pin::Pin;

    #[derive(Debug)]
    struct PendingSleep;

    impl AsyncSleep for PendingSleep {
        fn sleep(&self, _duration: Duration) -> aws_smithy_async::rt::sleep::Sleep {
            aws_smithy_async::rt::sleep::Sleep::new(std::future::pending())
        }
    }

    #[derive(Debug)]
    struct TestSpawner;

    impl DriverSpawner for TestSpawner {
        fn spawn(&self, driver: Pin<Box<dyn Future<Output = ()> + Send + 'static>>) {
            drop(driver);
        }
    }

    fn partition(index: usize) -> Partition {
        Partition::new(PartitionId::from_index(index), TestSpawner)
    }

    #[test]
    fn builder_defaults_and_nested_option_setters_are_distinct() {
        let mut builder = Builder::default();
        assert_eq!(None, builder.idle_timeout);
        assert_eq!(None, builder.tcp_keepalive);
        assert!(builder.tcp_nodelay);
        assert_eq!(None, builder.max_connections_per_host);
        assert!(builder.partitions.is_none());

        builder.set_idle_timeout(Some(None));
        builder.set_tcp_keepalive(Some(None));
        builder.set_tcp_nodelay(false);
        builder.set_max_connections_per_host(Some(3));
        builder.set_partitions(Some(vec![partition(7)]));
        assert_eq!(Some(None), builder.idle_timeout);
        assert_eq!(Some(None), builder.tcp_keepalive);
        assert!(!builder.tcp_nodelay);
        assert_eq!(Some(3), builder.max_connections_per_host);
        assert_eq!(1, builder.partitions.as_ref().unwrap().len());

        builder.set_idle_timeout(None);
        builder.set_tcp_keepalive(None);
        builder.set_max_connections_per_host(None);
        builder.set_partitions(None);
        assert_eq!(None, builder.idle_timeout);
        assert_eq!(None, builder.tcp_keepalive);
        assert_eq!(None, builder.max_connections_per_host);
        assert!(builder.partitions.is_none());
    }

    #[test]
    fn terminal_build_resolves_the_default_idle_timeout() {
        let pool = Builder::default()
            .sleep_impl(PendingSleep)
            .build_http()
            .unwrap();

        assert_eq!(
            Some(Duration::from_secs(90)),
            pool.inner.config.idle_timeout
        );
    }

    #[test]
    #[cfg(any(
        target_os = "android",
        target_os = "fuchsia",
        target_os = "illumos",
        target_os = "ios",
        target_os = "linux",
        target_os = "macos",
        target_os = "solaris",
        target_os = "tvos",
        target_os = "visionos",
        target_os = "watchos",
    ))]
    fn terminal_build_rejects_interface_names_with_nul_bytes() {
        let error = Builder::default()
            .partitions([partition(7).interface("eth\0invalid")])
            .build_http()
            .unwrap_err();
        assert_eq!(
            "partition PartitionId(7) has an invalid network-interface name",
            error.to_string()
        );
    }

    #[test]
    fn terminal_build_rejects_zero_connection_limit() {
        let error = Builder::default()
            .max_connections_per_host(0)
            .build_http()
            .unwrap_err();
        assert_eq!(
            "max_connections_per_host must be greater than zero",
            error.to_string()
        );
    }

    #[test]
    fn terminal_build_rejects_empty_explicit_partition_set() {
        let error = Builder::default().partitions([]).build_http().unwrap_err();
        assert_eq!(
            "explicit partition set must not be empty",
            error.to_string()
        );
    }

    #[test]
    fn terminal_build_rejects_duplicate_partition_ids() {
        let error = Builder::default()
            .partitions([partition(7), partition(7)])
            .build_http()
            .unwrap_err();
        assert_eq!(
            "duplicate partition identifier: PartitionId(7)",
            error.to_string()
        );
    }

    #[test]
    fn terminal_build_rejects_reserved_anonymous_partition_id() {
        let error = Builder::default()
            .partitions([Partition::new(PartitionId::ANONYMOUS, TestSpawner)])
            .build_http()
            .unwrap_err();
        assert_eq!(
            "the anonymous partition identifier is reserved",
            error.to_string()
        );
    }
}
