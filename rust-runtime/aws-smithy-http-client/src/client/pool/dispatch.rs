/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Request routing and protocol dispatch.
//!
//! This module is the request entry point after a [`super::Client`] has
//! resolved its partition. It rejects unsupported request forms, resolves the
//! origin cell and partition runtime, and owns acquisition across protocol
//! selection. Child modules prepare protocol wire forms and retain response
//! ownership.

mod h1;
mod h2;

use self::h1::H1DispatchResult;
use self::h2::H2DispatchResult;
use super::admission::ProtocolRequirement;
use super::cell::{AcquisitionEvent, AcquisitionResult, OriginCell, WaiterId};
use super::establish::{self, TransportTimeout};
use super::partition::DriverSpawner;
use super::registry::PartitionState;
use super::{ConnectionPool, PoolInner};
use crate::sync::Arc;
use aws_smithy_runtime_api::client::result::ConnectorError;
use aws_smithy_types::body::SdkBody;
use http_1x::{Method, Request, Response, Uri, Version};
use std::future::poll_fn;
use std::sync::Arc as StdArc;

/// Stable state shared by acquisition and protocol dispatch for one request.
///
/// This context owns ordinary shared configuration and identities. Linear
/// acquisition values, such as an establishment permit or an exclusive
/// protocol handle, remain separate arguments so their transfer is explicit.
#[derive(Clone)]
pub(super) struct AcquisitionContext {
    /// Shared pool policy and transport construction.
    pub(super) pool: Arc<PoolInner>,
    /// Partition from which the request was issued.
    pub(super) partition: Arc<PartitionState>,
    /// Partition-local state for the request's canonical origin.
    pub(super) cell: Arc<OriginCell>,
    /// Absolute request URI retained across protocol wire-form changes.
    pub(super) absolute_uri: Uri,
    /// Runtime that owns establishment, drivers, and pending return work.
    pub(super) owner_spawner: StdArc<dyn DriverSpawner>,
    /// Transport connection timeout configured for this operation.
    pub(super) connect_timeout: Option<TransportTimeout>,
}

impl ConnectionPool {
    /// Resolves common acquisition state and dispatches one request.
    pub(super) async fn send_request(
        &self,
        partition: Arc<PartitionState>,
        request: Request<SdkBody>,
        connect_timeout: Option<TransportTimeout>,
    ) -> Result<Response<SdkBody>, ConnectorError> {
        validate_request_before_acquisition(&request)
            .map_err(|error| ConnectorError::user(error.into()))?;

        let absolute_uri = request.uri().clone();
        let cell = self
            .inner
            .registry
            .resolve_cell(&partition, &absolute_uri)
            .map_err(|error| ConnectorError::user(error.into()))?;
        tracing::trace!(
            request_partition = ?partition.id(),
            origin_scheme = %cell.id().origin().scheme(),
            origin_host = cell.id().origin().host(),
            origin_port = ?cell.id().origin().port(),
            "request resolved to connection-pool cell"
        );

        let owner_spawner = partition
            .owner_spawner()
            .map_err(|error| ConnectorError::user(error.into()))?;
        partition.ensure_maintenance_started(owner_spawner.as_ref());

        let context = AcquisitionContext {
            pool: self.inner.clone(),
            partition,
            cell,
            absolute_uri,
            owner_spawner,
            connect_timeout,
        };

        self.dispatch_acquired(context, request).await
    }

    /// Acquires a compatible protocol value and dispatches the request.
    ///
    /// Reacquisition is allowed only while a protocol dispatcher returns the
    /// original request envelope.
    async fn dispatch_acquired(
        &self,
        context: AcquisitionContext,
        mut request: Request<SdkBody>,
    ) -> Result<Response<SdkBody>, ConnectorError> {
        let requirement = if request.version() == Version::HTTP_2 {
            ProtocolRequirement::H2Required
        } else {
            ProtocolRequirement::H1Compatible
        };

        loop {
            match Self::acquire(&context, requirement).await? {
                AcquisitionResult::H1(selection) => {
                    match self.dispatch_h1(&context, request, selection).await? {
                        H1DispatchResult::Response(response) => return Ok(response),
                        H1DispatchResult::Reacquire(returned) => request = returned,
                    }
                }
                AcquisitionResult::H2(activation) => {
                    match self.dispatch_h2(request, activation).await? {
                        H2DispatchResult::Response(response) => return Ok(response),
                        H2DispatchResult::Reacquire(returned) => request = returned,
                    }
                }
                AcquisitionResult::Failed(_) => {
                    unreachable!("failed acquisition returned as a successful result")
                }
            }
        }
    }

    /// Acquires one protocol value compatible with the request.
    ///
    /// Local reusable values complete immediately. Otherwise one waiter
    /// remains registered while it receives a reusable protocol value, an
    /// establishment failure, or authority to start establishment.
    async fn acquire(
        context: &AcquisitionContext,
        requirement: ProtocolRequirement,
    ) -> Result<AcquisitionResult, ConnectorError> {
        if let Some(activation) = OriginCell::select_h2(&context.cell) {
            tracing::trace!(
                connection_id = %activation.connection().id(),
                request_partition = ?context.partition.id(),
                connection_partition = ?activation.connection().owner_partition(),
                origin_scheme = %context.cell.id().origin().scheme(),
                origin_host = context.cell.id().origin().host(),
                origin_port = ?context.cell.id().origin().port(),
                "HTTP/2 pool hit; activating local generation"
            );
            return Ok(AcquisitionResult::H2(activation));
        }
        if requirement == ProtocolRequirement::H1Compatible {
            if let Some(selection) = OriginCell::select_h1(&context.cell) {
                tracing::trace!(
                    connection_id = %selection.connection_id(),
                    request_partition = ?context.partition.id(),
                    connection_partition = ?selection.connection().owner_partition(),
                    origin_scheme = %selection.connection().info().origin().scheme(),
                    origin_host = selection.connection().info().origin().host(),
                    origin_port = ?selection.connection().info().origin().port(),
                    "HTTP/1 pool hit; reusing idle connection"
                );
                return Ok(AcquisitionResult::H1(selection));
            }
        }

        let waiter = OriginCell::register_waiter(&context.cell, requirement);
        tracing::trace!(
            request_partition = ?context.partition.id(),
            origin_scheme = %context.cell.id().origin().scheme(),
            origin_host = context.cell.id().origin().host(),
            origin_port = ?context.cell.id().origin().port(),
            protocol_requirement = ?requirement,
            "connection acquisition queued"
        );
        let mut waiter_guard = WaiterGuard::new(context.cell.clone(), waiter);
        loop {
            match poll_fn(|cx| context.cell.poll_waiter(waiter, cx)).await {
                AcquisitionEvent::Complete(AcquisitionResult::H1(selection)) => {
                    waiter_guard.disarm();
                    return Ok(AcquisitionResult::H1(selection));
                }
                AcquisitionEvent::Complete(AcquisitionResult::H2(activation)) => {
                    waiter_guard.disarm();
                    tracing::trace!(
                        connection_id = %activation.connection().id(),
                        request_partition = ?context.partition.id(),
                        connection_partition = ?activation.connection().owner_partition(),
                        origin_scheme = %context.cell.id().origin().scheme(),
                        origin_host = context.cell.id().origin().host(),
                        origin_port = ?context.cell.id().origin().port(),
                        "HTTP/2 acquisition completed"
                    );
                    return Ok(AcquisitionResult::H2(activation));
                }
                AcquisitionEvent::Complete(AcquisitionResult::Failed(error)) => {
                    waiter_guard.disarm();
                    return Err(error);
                }
                AcquisitionEvent::Establish(permit) => {
                    tracing::trace!(
                        request_partition = ?context.partition.id(),
                        origin_scheme = %context.cell.id().origin().scheme(),
                        origin_host = context.cell.id().origin().host(),
                        origin_port = ?context.cell.id().origin().port(),
                        "connection establishment starting"
                    );
                    let attempt = establish::establish(context.clone(), waiter, permit);
                    let completion =
                        EstablishmentCompletionGuard::new(context.cell.clone(), waiter);
                    context.owner_spawner.spawn(Box::pin(async move {
                        let mut completion = completion;
                        if !completion.start() {
                            drop(attempt);
                            completion.disarm();
                            return;
                        }
                        match attempt.await {
                            establish::EstablishmentOutcome::Complete(result) => {
                                completion.complete(result);
                            }
                            establish::EstablishmentOutcome::Transferred => completion.disarm(),
                        }
                    }));
                }
            }
        }
    }
}

/// Guarantees a terminal result for one submitted establishment attempt.
///
/// The guard enters the owner-runtime future before submission. Runtime task
/// drop reports failure to the waiter. A result that wins before the submitted
/// future starts disarms the guard without polling the connector.
struct EstablishmentCompletionGuard {
    cell: Arc<OriginCell>,
    waiter: WaiterId,
    active: bool,
}

impl EstablishmentCompletionGuard {
    fn new(cell: Arc<OriginCell>, waiter: WaiterId) -> Self {
        Self {
            cell,
            waiter,
            active: true,
        }
    }

    fn start(&self) -> bool {
        self.cell.start_establishment(self.waiter)
    }

    fn complete(mut self, result: AcquisitionResult) {
        self.active = false;
        self.cell.complete_establishment(self.waiter, result);
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for EstablishmentCompletionGuard {
    fn drop(&mut self) {
        if self.active {
            let error = ConnectorError::other(EstablishmentTaskDropped.into(), None);
            self.cell
                .complete_establishment(self.waiter, AcquisitionResult::Failed(error));
            tracing::debug!(
                request_partition = ?self.cell.id().partition(),
                connection_partition = ?self.cell.id().partition(),
                origin_scheme = %self.cell.id().origin().scheme(),
                origin_host = self.cell.id().origin().host(),
                origin_port = ?self.cell.id().origin().port(),
                "connection establishment task dropped"
            );
        }
    }
}

/// The owner runtime discarded an establishment future before completion.
#[derive(Debug)]
struct EstablishmentTaskDropped;

impl std::fmt::Display for EstablishmentTaskDropped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the owner-runtime connection establishment task was dropped")
    }
}

impl std::error::Error for EstablishmentTaskDropped {}

/// Cancels a request's waiter until it consumes a terminal acquisition result.
struct WaiterGuard {
    cell: Arc<OriginCell>,
    waiter: WaiterId,
    active: bool,
}

impl WaiterGuard {
    fn new(cell: Arc<OriginCell>, waiter: WaiterId) -> Self {
        Self {
            cell,
            waiter,
            active: true,
        }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for WaiterGuard {
    fn drop(&mut self) {
        if self.active {
            OriginCell::cancel_waiter(&self.cell, self.waiter);
        }
    }
}

/// Rejects request forms the pool cannot route before acquiring connection state.
fn validate_request_before_acquisition(
    request: &Request<SdkBody>,
) -> Result<(), RequestDispatchError> {
    match request.version() {
        Version::HTTP_11 | Version::HTTP_2 => Ok(()),
        Version::HTTP_10 if request.method() == Method::CONNECT => {
            Err(RequestDispatchError::ConnectOverHttp10)
        }
        Version::HTTP_10 => Ok(()),
        version => Err(RequestDispatchError::UnsupportedVersion(version)),
    }
}

/// A request rejected before Hyper accepts it for dispatch.
#[derive(Debug)]
enum RequestDispatchError {
    /// The absolute URI did not contain the authority required for `Host`.
    MissingAuthority,
    /// The derived `Host` value was not a valid HTTP header value.
    InvalidHostHeader(http_1x::header::InvalidHeaderValue),
    /// HTTP/1.0 cannot represent a CONNECT request accepted by this client.
    ConnectOverHttp10,
    /// The request selected an HTTP version this client cannot dispatch.
    UnsupportedVersion(Version),
}

impl std::fmt::Display for RequestDispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingAuthority => f.write_str("request URI must contain an authority"),
            Self::InvalidHostHeader(_) => {
                f.write_str("request URI authority is not a valid Host header")
            }
            Self::ConnectOverHttp10 => f.write_str("CONNECT is not supported over HTTP/1.0"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported HTTP version: {version:?}")
            }
        }
    }
}

impl std::error::Error for RequestDispatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidHostHeader(error) => Some(error),
            _ => None,
        }
    }
}
