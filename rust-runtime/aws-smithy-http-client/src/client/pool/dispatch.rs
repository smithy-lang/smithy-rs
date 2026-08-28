/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Request preparation, protocol selection, and dispatch.
//!
//! This module is the request entry point after a [`super::Client`] has
//! resolved its partition. It rejects unsupported request forms, resolves the
//! origin cell and partition runtime, and starts partition maintenance before
//! handing the request to a protocol-specific dispatcher. Connection
//! acquisition, wire-form preparation, and response ownership live in child
//! modules.

mod h1;

use super::handshake::ConnectTimeout;
use super::registry::PartitionState;
use super::ConnectionPool;
use crate::sync::Arc;
use aws_smithy_runtime_api::client::result::ConnectorError;
use aws_smithy_types::body::SdkBody;
use http_1x::{Method, Request, Response, Version};

impl ConnectionPool {
    /// Routes one request to the protocol-specific dispatcher.
    pub(super) async fn send_request(
        &self,
        partition: Arc<PartitionState>,
        request: Request<SdkBody>,
        connect_timeout: Option<ConnectTimeout>,
    ) -> Result<Response<SdkBody>, ConnectorError> {
        validate_request_before_acquisition(&request)
            .map_err(|error| ConnectorError::user(error.into()))?;

        let cell = self
            .inner
            .registry
            .resolve_cell(&partition, request.uri())
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
        partition.start_maintenance(owner_spawner.as_ref());

        self.send_h1_request(partition, cell, request, connect_timeout, owner_spawner)
            .await
    }
}

/// Rejects request forms the pool cannot route before acquiring connection state.
fn validate_request_before_acquisition(
    request: &Request<SdkBody>,
) -> Result<(), RequestPreparationError> {
    match request.version() {
        Version::HTTP_11 | Version::HTTP_2 => Ok(()),
        Version::HTTP_10 if request.method() == Method::CONNECT => {
            Err(RequestPreparationError::ConnectOverHttp10)
        }
        Version::HTTP_10 => Ok(()),
        version => Err(RequestPreparationError::UnsupportedVersion(version)),
    }
}

/// A request property rejected during protocol-neutral or HTTP/1 preparation.
#[derive(Debug)]
enum RequestPreparationError {
    /// The absolute URI did not contain the authority required for `Host`.
    MissingAuthority,
    /// The derived `Host` value was not a valid HTTP header value.
    InvalidHostHeader(http_1x::header::InvalidHeaderValue),
    /// HTTP/1.0 cannot represent a CONNECT request accepted by this client.
    ConnectOverHttp10,
    /// The request selected an HTTP version this client cannot dispatch.
    UnsupportedVersion(Version),
}

impl std::fmt::Display for RequestPreparationError {
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

impl std::error::Error for RequestPreparationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidHostHeader(error) => Some(error),
            _ => None,
        }
    }
}
