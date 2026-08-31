/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Transport construction below HTTP protocol establishment.

mod h1;
mod h2;

use super::cell::{AcquisitionResult, EstablishmentPermit, WaiterId};
use super::dispatch::AcquisitionContext;
use super::registry::PartitionState;
use super::PoolInner;
use crate::client::connect::{AsyncConn, BoxConn};
use crate::client::timeout::{self, TimeoutKind};
use aws_smithy_async::rt::sleep::SharedAsyncSleep;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::connection::ConnectionId;
use http_1x::Uri;
#[cfg(any(feature = "__rustls", feature = "s2n-tls"))]
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::future::{poll_fn, Future};
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::Arc as StdArc;
use std::time::Duration;
use tower::Service;

/// Timeout policy for one transport connection operation.
///
/// Connector readiness completes before this timeout starts. Keeping the
/// duration and timer together lets request acquisition pass one complete
/// timeout value into transport construction.
#[derive(Clone, Debug)]
pub(super) struct TransportTimeout {
    /// Maximum duration of the connector's connection future.
    duration: Duration,
    /// Runtime timer used to enforce the deadline.
    sleep: SharedAsyncSleep,
}

impl TransportTimeout {
    /// Creates a timeout enforced by the supplied runtime timer.
    pub(super) fn new(duration: Duration, sleep: SharedAsyncSleep) -> Self {
        Self { duration, sleep }
    }
}

/// Future returned by a type-erased transport factory.
type TransportFuture = Pin<Box<dyn Future<Output = Result<BoxConn, BoxError>> + Send + 'static>>;

/// Type-erased transport construction below HTTP protocol establishment.
///
/// A pool retains one factory that applies partition interface placement,
/// connector readiness, and the operation's connection timeout. The concrete
/// connector remains responsible for DNS, proxy, TLS, socket construction,
/// and transport metadata. Protocol establishment consumes the resulting
/// [`BoxConn`].
pub(super) trait TransportFactory: Send + Sync + 'static {
    /// Creates one partition-bound transport for `uri`.
    ///
    /// Connector readiness completes before `timeout` starts.
    fn connect(
        &self,
        partition: &PartitionState,
        uri: Uri,
        timeout: Option<TransportTimeout>,
    ) -> TransportFuture;
}

/// Generic connector factory erased at pool construction.
struct ServiceTransportFactory<F> {
    /// Builds a connector with the selected network-interface binding.
    connector_for_interface: F,
}

impl<F, C, IO> TransportFactory for ServiceTransportFactory<F>
where
    F: Fn(Option<&str>) -> C + Send + Sync + 'static,
    C: Service<Uri, Response = IO> + Send + 'static,
    C::Error: Into<BoxError>,
    C::Future: Send + 'static,
    IO: AsyncConn,
{
    fn connect(
        &self,
        partition: &PartitionState,
        uri: Uri,
        timeout: Option<TransportTimeout>,
    ) -> TransportFuture {
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

#[cfg(any(
    all(feature = "test-util", aws_sdk_unstable),
    all(test, feature = "rt-tokio")
))]
/// Erases a clonable connector for pool storage.
pub(super) fn transport_factory<C, IO>(connector: C) -> StdArc<dyn TransportFactory>
where
    C: Service<Uri, Response = IO> + Clone + Send + Sync + 'static,
    C::Error: Into<BoxError>,
    C::Future: Send + 'static,
    IO: AsyncConn,
{
    transport_factory_for_interface(move |_| connector.clone())
}

/// Erases a connector constructor that applies an optional interface binding.
///
/// The default HTTP connector consumes this value. Custom connectors use
/// `transport_factory` and ignore pool interface placement.
pub(super) fn transport_factory_for_interface<F, C, IO>(
    connector_for_interface: F,
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
    })
}

/// Erases and caches connectors whose construction carries TLS or other setup.
///
/// Interface placement is immutable for a partition. Caching by the exact
/// interface value keeps provider configuration and certificate loading off
/// the request path while preserving one connector per placement.
#[cfg(any(feature = "__rustls", feature = "s2n-tls"))]
pub(super) fn cached_transport_factory_for_interface<F, C, IO>(
    connector_for_interface: F,
) -> StdArc<dyn TransportFactory>
where
    F: Fn(Option<&str>) -> C + Send + Sync + 'static,
    C: Service<Uri, Response = IO> + Clone + Send + Sync + 'static,
    C::Error: Into<BoxError>,
    C::Future: Send + 'static,
    IO: AsyncConn,
{
    struct Cached<F, C> {
        factory: F,
        connectors: crate::sync::Mutex<HashMap<Option<String>, C>>,
    }

    impl<F, C, IO> TransportFactory for Cached<F, C>
    where
        F: Fn(Option<&str>) -> C + Send + Sync + 'static,
        C: Service<Uri, Response = IO> + Clone + Send + Sync + 'static,
        C::Error: Into<BoxError>,
        C::Future: Send + 'static,
        IO: AsyncConn,
    {
        fn connect(
            &self,
            partition: &PartitionState,
            uri: Uri,
            timeout: Option<TransportTimeout>,
        ) -> TransportFuture {
            let interface = partition.interface().map(|value| value.to_string());
            let mut connector = self
                .connectors
                .lock()
                .entry(interface.clone())
                .or_insert_with(|| (self.factory)(interface.as_deref()))
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
        connectors: crate::sync::Mutex::new(HashMap::new()),
    })
}

/// Result of one owner-runtime establishment task.
pub(super) enum EstablishmentOutcome {
    /// The launching waiter receives this terminal result.
    Complete(AcquisitionResult),
    /// Waiter completion transferred to an H2 flight or generation.
    Transferred,
}

/// Connects one transport and dispatches protocol establishment after ALPN.
pub(super) async fn establish(
    context: AcquisitionContext,
    waiter: WaiterId,
    permit: EstablishmentPermit,
) -> EstablishmentOutcome {
    let io = match context
        .pool
        .transport
        .connect(
            &context.partition,
            context.absolute_uri.clone(),
            context.connect_timeout.clone(),
        )
        .await
    {
        Ok(io) => io,
        Err(error) => {
            tracing::debug!(
                request_partition = ?context.partition.id(),
                connection_partition = ?context.cell.id().partition(),
                origin_scheme = %context.cell.id().origin().scheme(),
                origin_host = context.cell.id().origin().host(),
                origin_port = ?context.cell.id().origin().port(),
                error = ?error,
                "transport establishment failed"
            );
            return EstablishmentOutcome::Complete(AcquisitionResult::Failed(
                super::super::downcast_error(error),
            ));
        }
    };
    let connected = io.connected();
    let negotiated_h2 = connected.is_negotiated_h2();
    tracing::debug!(
        request_partition = ?context.partition.id(),
        connection_partition = ?context.cell.id().partition(),
        origin_scheme = %context.cell.id().origin().scheme(),
        origin_host = context.cell.id().origin().host(),
        origin_port = ?context.cell.id().origin().port(),
        negotiated_protocol = if negotiated_h2 { "HTTP/2" } else { "HTTP/1.1" },
        "transport protocol negotiated"
    );
    if negotiated_h2 {
        h2::establish_h2(context, waiter, permit, io, connected).await
    } else {
        EstablishmentOutcome::Complete(
            h1::establish_h1(context, permit, io, connected)
                .await
                .map(AcquisitionResult::H1)
                .unwrap_or_else(AcquisitionResult::Failed),
        )
    }
}

/// Mints one non-wrapping physical-connection identity.
fn next_connection_id(pool: &PoolInner) -> Result<ConnectionId, ConnectionIdExhausted> {
    let value = pool
        .next_connection_id
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .map_err(|_| ConnectionIdExhausted)?;
    Ok(ConnectionId::new(value))
}

/// The pool exhausted its monotonic physical-connection identity space.
#[derive(Debug)]
struct ConnectionIdExhausted;

impl fmt::Display for ConnectionIdExhausted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("connection identifier space exhausted")
    }
}

impl Error for ConnectionIdExhausted {}
