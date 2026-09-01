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
use http_1x::{header, Method, Request, Response, Uri, Version};
use std::future::poll_fn;
use std::sync::Arc as StdArc;

/// Replacement selections allowed after the initial HTTP/2 dispatch attempt.
const MAX_H2_REACQUISITIONS: usize = 2;

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
        let requirement = protocol_requirement(&request);
        let mut h2_reacquisitions = H2ReacquisitionBudget::default();

        loop {
            match Self::acquire(&context, requirement).await? {
                AcquisitionResult::H1(selection) => {
                    match self.dispatch_h1(&context, request, selection).await? {
                        H1DispatchResult::Response(response) => return Ok(response),
                        H1DispatchResult::Reacquire(returned) => request = returned,
                    }
                }
                AcquisitionResult::H2(activation) => {
                    match self.dispatch_h2(&context, request, activation).await? {
                        H2DispatchResult::Response(response) => return Ok(response),
                        H2DispatchResult::Reacquire(reacquisition) => {
                            let (returned, error) = reacquisition.into_parts();
                            if !h2_reacquisitions.admit_replacement() {
                                return Err(error);
                            }
                            request = returned;
                        }
                    }
                }
                AcquisitionResult::Failed(_) => {
                    unreachable!("failed acquisition returned as a successful result")
                }
                AcquisitionResult::Reacquire => {
                    unreachable!("internal reacquisition escaped the acquisition loop")
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
        'acquire: loop {
            if requirement.accepts_h2() {
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
            }
            if requirement.accepts_h1() {
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
                        OriginCell::service_h2_waiters(&context.cell);
                        OriginCell::service_peer_h2_waiters(&context.cell);
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
                    AcquisitionEvent::Complete(AcquisitionResult::Reacquire) => {
                        waiter_guard.disarm();
                        continue 'acquire;
                    }
                    AcquisitionEvent::Establish(permit) => {
                        tracing::trace!(
                            request_partition = ?context.partition.id(),
                            origin_scheme = %context.cell.id().origin().scheme(),
                            origin_host = context.cell.id().origin().host(),
                            origin_port = ?context.cell.id().origin().port(),
                            "connection establishment starting"
                        );
                        let attempt =
                            establish::establish(context.clone(), waiter, permit, requirement);
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
}

/// Per-request bound on replacement HTTP/2 selections.
#[derive(Default)]
struct H2ReacquisitionBudget {
    /// Replacement selections admitted after the initial selection.
    completed: usize,
}

impl H2ReacquisitionBudget {
    /// Returns whether one more replacement selection may proceed.
    fn admit_replacement(&mut self) -> bool {
        if self.completed >= MAX_H2_REACQUISITIONS {
            return false;
        }
        self.completed += 1;
        true
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
            let error = ConnectorError::io(EstablishmentTaskDropped.into());
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
    if request.version() == Version::HTTP_2 && request.method() == Method::CONNECT {
        return Err(RequestDispatchError::ExtendedConnectUnsupported);
    }
    if request.version() == Version::HTTP_2 && has_upgrade_semantics(request) {
        return Err(RequestDispatchError::UpgradeOverHttp2);
    }
    match request.version() {
        Version::HTTP_11 | Version::HTTP_2 => Ok(()),
        Version::HTTP_10 if request.method() == Method::CONNECT => {
            Err(RequestDispatchError::ConnectOverHttp10)
        }
        Version::HTTP_10 => Ok(()),
        version => Err(RequestDispatchError::UnsupportedVersion(version)),
    }
}

/// Returns the protocol capability needed to preserve the request's wire semantics.
fn protocol_requirement(request: &Request<SdkBody>) -> ProtocolRequirement {
    if request.version() == Version::HTTP_2 {
        ProtocolRequirement::H2Required
    } else if request.version() == Version::HTTP_10
        || request.method() == Method::CONNECT
        || has_upgrade_semantics(request)
    {
        ProtocolRequirement::H1Required
    } else {
        ProtocolRequirement::H1Compatible
    }
}

/// Returns whether the request asks HTTP/1 to switch protocols.
fn has_upgrade_semantics(request: &Request<SdkBody>) -> bool {
    request.headers().contains_key(header::UPGRADE)
        || request
            .headers()
            .get_all(header::CONNECTION)
            .iter()
            .any(|value| {
                value.to_str().is_ok_and(|value| {
                    value
                        .split(',')
                        .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
                })
            })
}

/// Marker for a `Host` header synthesized during an HTTP/1 dispatch attempt.
#[derive(Clone, Copy, Debug)]
struct H1HostHeaderInserted;

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
    /// Extended CONNECT requires HTTP/2 stream-lifecycle support.
    ExtendedConnectUnsupported,
    /// HTTP/1 upgrade semantics cannot be represented by this HTTP/2 path.
    UpgradeOverHttp2,
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
            Self::ExtendedConnectUnsupported => {
                f.write_str("extended CONNECT over HTTP/2 is not supported")
            }
            Self::UpgradeOverHttp2 => {
                f.write_str("HTTP/1 protocol upgrade semantics cannot use HTTP/2")
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

#[cfg(all(test, not(smithy_http_client_loom)))]
mod tests {
    use super::*;

    fn request(method: Method, version: Version) -> Request<SdkBody> {
        Request::builder()
            .method(method)
            .uri("https://example.com/resource")
            .version(version)
            .body(SdkBody::empty())
            .unwrap()
    }

    #[test]
    fn request_semantics_select_protocol_capability() {
        assert_eq!(
            ProtocolRequirement::H1Compatible,
            protocol_requirement(&request(Method::GET, Version::HTTP_11))
        );
        assert_eq!(
            ProtocolRequirement::H2Required,
            protocol_requirement(&request(Method::GET, Version::HTTP_2))
        );
        assert_eq!(
            ProtocolRequirement::H1Required,
            protocol_requirement(&request(Method::GET, Version::HTTP_10))
        );
        assert_eq!(
            ProtocolRequirement::H1Required,
            protocol_requirement(&request(Method::CONNECT, Version::HTTP_11))
        );

        let mut upgrade = request(Method::GET, Version::HTTP_11);
        upgrade
            .headers_mut()
            .insert(header::CONNECTION, "keep-alive, Upgrade".parse().unwrap());
        assert_eq!(
            ProtocolRequirement::H1Required,
            protocol_requirement(&upgrade)
        );
    }

    #[test]
    fn unsupported_h2_tunnel_and_upgrade_forms_are_rejected() {
        assert!(matches!(
            validate_request_before_acquisition(&request(Method::CONNECT, Version::HTTP_2)),
            Err(RequestDispatchError::ExtendedConnectUnsupported)
        ));

        let mut upgrade = request(Method::GET, Version::HTTP_2);
        upgrade
            .headers_mut()
            .insert(header::UPGRADE, "websocket".parse().unwrap());
        assert!(matches!(
            validate_request_before_acquisition(&upgrade),
            Err(RequestDispatchError::UpgradeOverHttp2)
        ));
    }

    #[test]
    fn h2_reacquisition_is_bounded_after_two_replacements() {
        let mut budget = H2ReacquisitionBudget::default();
        for _ in 0..MAX_H2_REACQUISITIONS {
            assert!(budget.admit_replacement());
        }
        assert!(!budget.admit_replacement());
    }
}
