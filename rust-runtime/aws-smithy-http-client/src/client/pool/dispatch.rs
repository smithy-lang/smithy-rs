/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Request routing and protocol dispatch.
//!
//! This module is the request entry point after a [`super::Client`] has
//! resolved its partition. It rejects unsupported request forms, resolves the
//! origin cell and partition runtime, and starts partition maintenance before
//! handing the request to a protocol-specific dispatcher. Connection
//! acquisition, wire-form preparation, and response ownership live in child
//! modules.

mod h1;

use super::cell::OriginCell;
use super::establish::TransportTimeout;
use super::partition::DriverSpawner;
use super::registry::PartitionState;
use super::{ConnectionPool, PoolInner};
use crate::sync::Arc;
use aws_smithy_runtime_api::client::result::ConnectorError;
use aws_smithy_types::body::SdkBody;
use http_1x::{Method, Request, Response, Uri, Version};
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

        self.send_h1_request(context, request).await
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
