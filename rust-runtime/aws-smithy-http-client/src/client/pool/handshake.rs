/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Transport connection and HTTP/1 handshake.

use super::cell::h1::{H1CloseHandle, H1DriverGuard, H1Selection, H1Sender};
use super::cell::{EstablishmentPermit, OriginCell};
use super::connection::{
    CloseReason, ConnectionInfo, ConnectionIo, ConnectionState, NegotiatedProtocol,
};
use super::partition::DriverSpawner;
use super::registry::PartitionState;
use super::PoolInner;
use crate::client::connect::{AsyncConn, BoxConn};
use crate::client::timeout::{self, TimeoutKind};
use crate::sync::Arc;
use aws_smithy_async::rt::sleep::SharedAsyncSleep;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::connection::ConnectionId;
use aws_smithy_runtime_api::client::result::ConnectorError;
use aws_smithy_types::body::SdkBody;
use http_1x::{Extensions, Uri};
use hyper_util::client::legacy::connect::HttpInfo;
use std::error::Error;
use std::fmt;
use std::future::{poll_fn, Future};
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::Arc as StdArc;
use std::time::Duration;
use tower::Service;

/// Connect timeout supplied by one Smithy operation.
#[derive(Clone, Debug)]
pub(super) struct ConnectTimeout {
    duration: Duration,
    sleep: SharedAsyncSleep,
}

impl ConnectTimeout {
    /// Pairs a configured duration with the runtime sleep implementation.
    pub(super) fn new(duration: Duration, sleep: SharedAsyncSleep) -> Self {
        Self { duration, sleep }
    }
}

type TransportFuture = Pin<Box<dyn Future<Output = Result<BoxConn, BoxError>> + Send + 'static>>;

/// Type-erased construction of one partition-bound transport operation.
pub(super) trait TransportFactory: Send + Sync + 'static {
    /// Polls connector readiness and starts one transport connection.
    fn connect(
        &self,
        partition: &PartitionState,
        uri: Uri,
        timeout: Option<ConnectTimeout>,
    ) -> TransportFuture;
}

/// Generic connector factory erased at pool construction.
struct ServiceTransportFactory<F> {
    make_connector: F,
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
        timeout: Option<ConnectTimeout>,
    ) -> TransportFuture {
        let mut connector =
            (self.make_connector)(partition.interface().map(|value| value.as_ref()));
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

/// Erases a clonable connector constructor for pool storage.
pub(super) fn transport_factory<F, C, IO>(make_connector: F) -> StdArc<dyn TransportFactory>
where
    F: Fn(Option<&str>) -> C + Send + Sync + 'static,
    C: Service<Uri, Response = IO> + Send + 'static,
    C::Error: Into<BoxError>,
    C::Future: Send + 'static,
    IO: AsyncConn,
{
    StdArc::new(ServiceTransportFactory { make_connector })
}

/// Connects, handshakes, installs, and starts the owner-partition driver.
pub(super) async fn establish_h1(
    pool: StdArc<PoolInner>,
    partition: Arc<PartitionState>,
    cell: Arc<OriginCell>,
    uri: Uri,
    permit: EstablishmentPermit,
    connect_timeout: Option<ConnectTimeout>,
) -> Result<H1Selection, ConnectorError> {
    let spawner = owner_spawner(&partition)?;
    tracing::debug!(
        owner_partition = ?partition.id(),
        origin = %uri,
        "starting HTTP/1 connection establishment"
    );
    let io = pool
        .transport
        .connect(&partition, uri, connect_timeout)
        .await
        .map_err(super::super::downcast_error)?;
    let connected = io.connected();
    if connected.is_negotiated_h2() {
        return Err(ConnectorError::user(UnsupportedNegotiatedProtocol.into()));
    }

    let mut extras = Extensions::new();
    connected.get_extras(&mut extras);
    let http_info = extras.get::<HttpInfo>();
    let id = next_connection_id(&pool)?;
    let info = ConnectionInfo::new(
        id,
        cell.id().origin().clone(),
        partition.id(),
        NegotiatedProtocol::Http1,
        http_info.map(HttpInfo::local_addr),
        http_info.map(HttpInfo::remote_addr),
        connected.is_proxied(),
    );
    let (connection, physical) = match permit.into_lease() {
        Some(lease) => ConnectionState::bounded(info, lease),
        None => ConnectionState::unbounded(info),
    };
    let io = ConnectionIo::new(io, physical);

    let (sender, driver) = match hyper::client::conn::http1::Builder::new()
        .handshake::<_, SdkBody>(io)
        .await
    {
        Ok(established) => established,
        Err(error) => {
            connection.logical_close(CloseReason::ProtocolClosed);
            return Err(super::super::downcast_error(Box::new(error)));
        }
    };

    let selection =
        OriginCell::install_selected_h1(&cell, connection.clone(), H1Sender::from_hyper(sender));
    tracing::debug!(
        connection_id = %id,
        owner_partition = ?partition.id(),
        origin_scheme = %cell.id().origin().scheme(),
        origin_host = cell.id().origin().host(),
        origin_port = ?cell.id().origin().port(),
        "HTTP/1 connection established"
    );
    let driver_guard = H1DriverGuard::new(H1CloseHandle::new(&cell, &connection));
    spawner.spawn(Box::pin(async move {
        let result = driver.with_upgrades().await;
        if let Err(error) = result {
            tracing::debug!(connection_id = %id, error = ?error, "HTTP/1 connection driver stopped");
        }
        driver_guard.protocol_closed();
    }));
    Ok(selection)
}

/// Mints one non-wrapping connection identity.
fn next_connection_id(pool: &PoolInner) -> Result<ConnectionId, ConnectorError> {
    let value = pool
        .next_connection_id
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .map_err(|_| ConnectorError::other(ConnectionIdExhausted.into(), None))?;
    Ok(ConnectionId::new(value))
}

/// Resolves the retained explicit spawner or captures the anonymous runtime.
pub(super) fn owner_spawner(
    partition: &PartitionState,
) -> Result<StdArc<dyn DriverSpawner>, ConnectorError> {
    #[cfg(feature = "rt-tokio")]
    {
        Ok(partition
            .driver_spawner_with(|| StdArc::new(super::partition::TokioDriverSpawner::current())))
    }

    #[cfg(not(feature = "rt-tokio"))]
    {
        if partition.id().is_anonymous() {
            return Err(ConnectorError::user(MissingAnonymousRuntime.into()));
        }
        Ok(partition.driver_spawner_with(|| {
            unreachable!("an explicit partition always has a driver spawner")
        }))
    }
}

#[derive(Debug)]
struct UnsupportedNegotiatedProtocol;

impl fmt::Display for UnsupportedNegotiatedProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the HTTP/1 pool path cannot install an HTTP/2 transport")
    }
}

impl Error for UnsupportedNegotiatedProtocol {}

#[derive(Debug)]
struct ConnectionIdExhausted;

impl fmt::Display for ConnectionIdExhausted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("connection identifier space exhausted")
    }
}

impl Error for ConnectionIdExhausted {}

#[cfg(not(feature = "rt-tokio"))]
#[derive(Debug)]
struct MissingAnonymousRuntime;

#[cfg(not(feature = "rt-tokio"))]
impl fmt::Display for MissingAnonymousRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the anonymous connection-pool partition requires the `rt-tokio` feature")
    }
}

#[cfg(not(feature = "rt-tokio"))]
impl Error for MissingAnonymousRuntime {}
