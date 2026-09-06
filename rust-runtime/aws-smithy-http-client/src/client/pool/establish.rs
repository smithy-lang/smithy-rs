/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Transport connection followed by HTTP protocol establishment.
//!
//! [`transport`] creates connected I/O and reports the negotiated protocol.
//! This module then routes that transport through the matching Hyper handshake,
//! creates the protocol-neutral connection identity, and returns or transfers
//! completion ownership for the launching waiter.

mod h1;
mod h2;
mod transport;

#[cfg(any(feature = "__rustls", feature = "s2n-tls"))]
pub(super) use transport::from_cached_interface_connector;
#[cfg(any(
    all(feature = "test-util", aws_sdk_unstable),
    all(test, feature = "rt-tokio")
))]
pub(super) use transport::from_connector;
pub(super) use transport::{from_interface_connector, TransportFactory, TransportTimeout};

use self::transport::TransportConnectContext;
use super::admission::ProtocolRequirement;
use super::cell::{AcquisitionResult, EstablishmentPermit, WaiterId};
use super::dispatch::AcquisitionContext;
use super::PoolInner;
use aws_smithy_runtime_api::client::connection::ConnectionId;
use aws_smithy_runtime_api::client::result::ConnectorError;
use std::error::Error;
use std::fmt;
use std::sync::atomic::Ordering;

/// Result of one owner-runtime establishment task.
pub(super) enum EstablishmentOutcome {
    /// The launching waiter receives this terminal result.
    Complete(AcquisitionResult),
    /// An H2 flight or generation now owns the launching waiter's completion.
    WaiterCompletionTransferred,
}

/// Connects one transport and dispatches protocol establishment after ALPN.
pub(super) async fn establish(
    context: AcquisitionContext,
    waiter: WaiterId,
    permit: EstablishmentPermit,
    requirement: ProtocolRequirement,
) -> EstablishmentOutcome {
    let connect = TransportConnectContext::new(
        &context.partition,
        context.absolute_uri.clone(),
        context.connect_timeout.clone(),
        requirement,
    );
    let io = match context.pool.transport.connect(connect).await {
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
    if negotiated_h2 && !requirement.accepts_h2() {
        drop(io);
        drop(permit);
        return EstablishmentOutcome::Complete(AcquisitionResult::Failed(
            negotiated_protocol_mismatch(requirement),
        ));
    }

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
/// An established transport selected a protocol incompatible with the request.
#[derive(Debug)]
struct NegotiatedProtocolMismatch {
    requirement: ProtocolRequirement,
}

impl fmt::Display for NegotiatedProtocolMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "transport negotiated HTTP/2, which does not satisfy {:?} request semantics",
            self.requirement
        )
    }
}

impl Error for NegotiatedProtocolMismatch {}

fn negotiated_protocol_mismatch(requirement: ProtocolRequirement) -> ConnectorError {
    ConnectorError::other(NegotiatedProtocolMismatch { requirement }.into(), None)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiated_protocol_mismatch_is_not_a_user_error() {
        let error = negotiated_protocol_mismatch(ProtocolRequirement::H1Required);

        assert!(error.is_other());
        assert!(!error.is_user());
    }
}
