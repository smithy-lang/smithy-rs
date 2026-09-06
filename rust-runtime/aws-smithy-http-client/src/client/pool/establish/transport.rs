/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Transport connection policy below HTTP protocol establishment.
//!
//! This module owns the boundary from a pool connection attempt to connected
//! I/O with a negotiated protocol. [`TransportConnectContext`] keeps placement,
//! timeout, and ALPN inputs together. A factory may delegate DNS, proxy, TLS,
//! and ALPN to an injected connector, but it must report whether an HTTP/1-only
//! attempt is enforceable before admission may reclaim an idle H2 connection.
//!
//! Cached factories retain configured connector services, not connections or
//! DNS results. Network I/O begins only when [`TransportFactory::connect`] is
//! called.

use super::super::admission::ProtocolRequirement;
use super::super::registry::PartitionState;
use crate::client::connect::{AsyncConn, BoxConn};
use crate::client::timeout::{self, TimeoutKind};
use aws_smithy_async::rt::sleep::SharedAsyncSleep;
use aws_smithy_runtime_api::box_error::BoxError;
use http_1x::Uri;
use std::future::{poll_fn, Future};
use std::pin::Pin;
use std::sync::Arc as StdArc;
use std::time::Duration;
use tower::Service;

/// Timeout policy for one transport connection operation.
///
/// Connector readiness completes before this timeout starts. Keeping the
/// duration and timer together lets request acquisition pass one complete
/// timeout value into transport construction.
#[derive(Clone, Debug)]
pub(in crate::client::pool) struct TransportTimeout {
    /// Maximum duration of the connector's connection future.
    duration: Duration,
    /// Runtime timer used to enforce the deadline.
    sleep: SharedAsyncSleep,
}

impl TransportTimeout {
    /// Creates a timeout enforced by the supplied runtime timer.
    pub(in crate::client::pool) fn new(duration: Duration, sleep: SharedAsyncSleep) -> Self {
        Self { duration, sleep }
    }
}

/// Static ALPN protocol offer used for one transport connection.
pub(in crate::client::pool) type AlpnProtocols = &'static [&'static [u8]];

/// Default offer for requests that may use HTTP/2.
const HTTP_ALPN_PROTOCOLS: AlpnProtocols = &[b"h2", b"http/1.1"];

/// Narrowed offer for requests that require HTTP/1 wire semantics.
const HTTP1_ALPN_PROTOCOLS: AlpnProtocols = &[b"http/1.1"];

/// Inputs needed to create one transport connection.
///
/// This context is the transport-attempt boundary for DNS, socket, proxy, TLS,
/// and ALPN lifecycle observation. It carries no installed connection state
/// because an attempt may fail before a connection exists.
pub(in crate::client::pool) struct TransportConnectContext<'a> {
    /// Partition whose placement policy owns the connection attempt.
    partition: &'a PartitionState,
    /// Absolute URI passed to the connector contract.
    uri: Uri,
    /// Optional deadline for the connector's connection future.
    timeout: Option<TransportTimeout>,
    /// Protocols the default TLS connector may advertise for this attempt.
    alpn_protocols: AlpnProtocols,
}

impl<'a> TransportConnectContext<'a> {
    /// Creates transport inputs after request protocol classification.
    pub(super) fn new(
        partition: &'a PartitionState,
        uri: Uri,
        timeout: Option<TransportTimeout>,
        requirement: ProtocolRequirement,
    ) -> Self {
        Self {
            partition,
            uri,
            timeout,
            alpn_protocols: alpn_protocols(requirement),
        }
    }
}

/// Future returned by a type-erased transport factory.
type TransportFuture = Pin<Box<dyn Future<Output = Result<BoxConn, BoxError>> + Send + 'static>>;

/// Type-erased transport construction below HTTP protocol establishment.
///
/// A pool retains one factory. The concrete connector remains responsible for
/// the transport stages it implements and for returning connector metadata.
/// Protocol establishment consumes the resulting [`BoxConn`].
pub(in crate::client::pool) trait TransportFactory:
    Send + Sync + 'static
{
    /// Returns whether an H1-required attempt is guaranteed to negotiate H1.
    fn guarantees_http1(&self) -> bool;

    /// Creates one partition-bound transport.
    ///
    /// Connector readiness completes before the connection timeout starts.
    fn connect(&self, context: TransportConnectContext<'_>) -> TransportFuture;
}

/// Connector factory whose concrete service is selected by interface.
struct ServiceTransportFactory<F> {
    /// Builds a connector with the selected network-interface binding.
    connector_for_interface: F,
    /// Whether every H1-required connection is guaranteed to negotiate H1.
    guarantees_http1: bool,
}

impl<F, C, IO> TransportFactory for ServiceTransportFactory<F>
where
    F: Fn(Option<&str>) -> C + Send + Sync + 'static,
    C: Service<Uri, Response = IO> + Send + 'static,
    C::Error: Into<BoxError>,
    C::Future: Send + 'static,
    IO: AsyncConn,
{
    fn guarantees_http1(&self) -> bool {
        self.guarantees_http1
    }

    fn connect(&self, context: TransportConnectContext<'_>) -> TransportFuture {
        let TransportConnectContext {
            partition,
            uri,
            timeout,
            alpn_protocols: _alpn_protocols,
        } = context;
        let interface = partition.interface().map(|interface| interface.as_ref());
        let mut connector = (self.connector_for_interface)(interface);
        Box::pin(async move {
            // Readiness follows the existing connector contract and is not timed.
            poll_fn(|cx| connector.poll_ready(cx))
                .await
                .map_err(Into::into)?;
            let connect = connector.call(uri);
            let io = timeout::maybe_timeout_future(
                connect,
                timeout.as_ref().map(|timeout| timeout.duration),
                timeout.as_ref().map(|timeout| &timeout.sleep),
                TimeoutKind::Connect,
            )
            .await?;
            Ok(Box::new(io) as BoxConn)
        })
    }
}

/// Erases an injected connector that owns placement and protocol negotiation.
///
/// The pool cannot apply its interface or ALPN inputs to this connector. It is
/// therefore conservative about H2 reclaim for H1-required demand.
#[cfg(any(
    all(feature = "test-util", aws_sdk_unstable),
    all(test, feature = "rt-tokio")
))]
pub(in crate::client::pool) fn from_connector<C, IO>(connector: C) -> StdArc<dyn TransportFactory>
where
    C: Service<Uri, Response = IO> + Clone + Send + Sync + 'static,
    C::Error: Into<BoxError>,
    C::Future: Send + 'static,
    IO: AsyncConn,
{
    service_factory(move |_| connector.clone(), false)
}

/// Erases a cleartext connector constructor that applies interface placement.
pub(in crate::client::pool) fn from_interface_connector<F, C, IO>(
    connector_for_interface: F,
) -> StdArc<dyn TransportFactory>
where
    F: Fn(Option<&str>) -> C + Send + Sync + 'static,
    C: Service<Uri, Response = IO> + Send + 'static,
    C::Error: Into<BoxError>,
    C::Future: Send + 'static,
    IO: AsyncConn,
{
    service_factory(connector_for_interface, true)
}

/// Erases a service connector and its HTTP/1 negotiation guarantee.
fn service_factory<F, C, IO>(
    connector_for_interface: F,
    guarantees_http1: bool,
) -> StdArc<dyn TransportFactory>
where
    F: Fn(Option<&str>) -> C + Send + Sync + 'static,
    C: Service<Uri, Response = IO> + Send + 'static,
    C::Error: Into<BoxError>,
    C::Future: Send + 'static,
    IO: AsyncConn,
{
    StdArc::new(ServiceTransportFactory {
        connector_for_interface,
        guarantees_http1,
    })
}

/// Erases and caches configured TLS connector services.
///
/// Interface placement is immutable for a partition. The first request for
/// each `(interface, ALPN offer)` constructs its connector while holding the
/// cache lock; later requests clone that retained connector. Provider
/// configuration and certificate loading therefore occur at most once for
/// each placement and offer rather than once per connection.
#[cfg(any(feature = "__rustls", feature = "s2n-tls"))]
pub(in crate::client::pool) fn from_cached_interface_connector<F, C, IO>(
    connector_for_interface: F,
    guarantees_http1: bool,
) -> StdArc<dyn TransportFactory>
where
    F: Fn(Option<&str>, AlpnProtocols) -> C + Send + Sync + 'static,
    C: Service<Uri, Response = IO> + Clone + Send + Sync + 'static,
    C::Error: Into<BoxError>,
    C::Future: Send + 'static,
    IO: AsyncConn,
{
    struct Cached<F, C> {
        factory: F,
        guarantees_http1: bool,
        connectors:
            crate::sync::Mutex<std::collections::HashMap<(Option<String>, AlpnProtocols), C>>,
    }

    impl<F, C, IO> TransportFactory for Cached<F, C>
    where
        F: Fn(Option<&str>, AlpnProtocols) -> C + Send + Sync + 'static,
        C: Service<Uri, Response = IO> + Clone + Send + Sync + 'static,
        C::Error: Into<BoxError>,
        C::Future: Send + 'static,
        IO: AsyncConn,
    {
        fn guarantees_http1(&self) -> bool {
            self.guarantees_http1
        }

        fn connect(&self, context: TransportConnectContext<'_>) -> TransportFuture {
            let TransportConnectContext {
                partition,
                uri,
                timeout,
                alpn_protocols,
            } = context;
            let interface = partition.interface().map(|value| value.to_string());
            let mut connector = self
                .connectors
                .lock()
                .entry((interface.clone(), alpn_protocols))
                .or_insert_with(|| (self.factory)(interface.as_deref(), alpn_protocols))
                .clone();
            Box::pin(async move {
                poll_fn(|cx| connector.poll_ready(cx))
                    .await
                    .map_err(Into::into)?;
                let connect = connector.call(uri);
                let io = timeout::maybe_timeout_future(
                    connect,
                    timeout.as_ref().map(|timeout| timeout.duration),
                    timeout.as_ref().map(|timeout| &timeout.sleep),
                    TimeoutKind::Connect,
                )
                .await?;
                Ok(Box::new(io) as BoxConn)
            })
        }
    }

    StdArc::new(Cached {
        factory: connector_for_interface,
        guarantees_http1,
        connectors: crate::sync::Mutex::new(std::collections::HashMap::new()),
    })
}

/// Selects the ALPN offer that preserves the request's wire semantics.
fn alpn_protocols(requirement: ProtocolRequirement) -> AlpnProtocols {
    match requirement {
        ProtocolRequirement::H1Required => HTTP1_ALPN_PROTOCOLS,
        ProtocolRequirement::H1Compatible | ProtocolRequirement::H2Required => HTTP_ALPN_PROTOCOLS,
    }
}
