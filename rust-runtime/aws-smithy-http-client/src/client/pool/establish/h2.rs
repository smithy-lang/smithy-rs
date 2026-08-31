/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP/2 flight convergence, handshake, and driver installation.

use super::super::cell::h2::{H2CloseHandle, H2DriverGuard, H2FlightId, H2FlightInstall, H2Sender};
use super::super::cell::{AcquisitionResult, EstablishmentPermit, OriginCell, WaiterId};
use super::super::connection::{
    CloseReason, ConnectionInfo, ConnectionIo, ConnectionState, NegotiatedProtocol,
};
use super::super::dispatch::AcquisitionContext;
use super::super::partition::DriverSpawner;
use super::{next_connection_id, EstablishmentOutcome};
use crate::client::connect::BoxConn;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::result::ConnectorError;
use aws_smithy_types::body::SdkBody;
use hyper::rt::Executor;
use hyper_util::client::legacy::connect::Connected;
use std::future::Future;
use std::sync::Arc as StdArc;

/// Hyper executor that keeps spawned HTTP/2 work on the connection partition.
#[derive(Clone, Debug)]
struct PartitionExecutor {
    spawner: StdArc<dyn DriverSpawner>,
}

impl<F> Executor<F> for PartitionExecutor
where
    F: Future<Output = ()> + Send + 'static,
{
    fn execute(&self, future: F) {
        self.spawner.spawn(Box::pin(future));
    }
}

/// Settles a flight if its owner-runtime task is discarded.
struct FlightCompletionGuard {
    cell: crate::sync::Arc<OriginCell>,
    flight: H2FlightId,
    active: bool,
}

impl FlightCompletionGuard {
    fn new(cell: crate::sync::Arc<OriginCell>, flight: H2FlightId) -> Self {
        Self {
            cell,
            flight,
            active: true,
        }
    }

    fn disarm(&mut self) {
        self.active = false;
    }

    fn fail(&mut self, error: BoxError) {
        self.active = false;
        fail_participants(&self.cell, self.flight, StdArc::new(error));
    }
}

impl Drop for FlightCompletionGuard {
    fn drop(&mut self) {
        if self.active {
            fail_participants(
                &self.cell,
                self.flight,
                StdArc::new(Box::new(std::io::Error::other(
                    "HTTP/2 establishment task was dropped",
                ))),
            );
        }
    }
}

/// Converges after ALPN and performs at most one HTTP/2 handshake per cell.
pub(super) async fn establish_h2(
    context: AcquisitionContext,
    waiter: WaiterId,
    permit: EstablishmentPermit,
    io: BoxConn,
    connected: Connected,
) -> EstablishmentOutcome {
    loop {
        match context.cell.install_or_join_h2_flight(waiter) {
            H2FlightInstall::Accepting(generation) => {
                tracing::trace!(
                    request_partition = ?context.partition.id(),
                    connection_partition = ?context.cell.id().partition(),
                    origin_scheme = %context.cell.id().origin().scheme(),
                    origin_host = context.cell.id().origin().host(),
                    origin_port = ?context.cell.id().origin().port(),
                    h2_generation = ?generation,
                    "HTTP/2 establishment found an accepting generation"
                );
                if !OriginCell::join_h2_generation(&context.cell, waiter, generation) {
                    continue;
                }
                drop((io, permit));
                return EstablishmentOutcome::Transferred;
            }
            H2FlightInstall::Joined => {
                tracing::trace!(
                    request_partition = ?context.partition.id(),
                    connection_partition = ?context.cell.id().partition(),
                    origin_scheme = %context.cell.id().origin().scheme(),
                    origin_host = context.cell.id().origin().host(),
                    origin_port = ?context.cell.id().origin().port(),
                    "HTTP/2 establishment joined the active flight"
                );
                drop((io, permit));
                return EstablishmentOutcome::Transferred;
            }
            H2FlightInstall::Driver(flight) => {
                tracing::trace!(
                    request_partition = ?context.partition.id(),
                    connection_partition = ?context.cell.id().partition(),
                    origin_scheme = %context.cell.id().origin().scheme(),
                    origin_host = context.cell.id().origin().host(),
                    origin_port = ?context.cell.id().origin().port(),
                    h2_flight = ?flight,
                    "HTTP/2 establishment started a flight"
                );
                drive_flight(context, flight, permit, io, connected).await;
                return EstablishmentOutcome::Transferred;
            }
        }
    }
}

/// Handshakes, installs, and publishes one winning flight.
async fn drive_flight(
    context: AcquisitionContext,
    flight: H2FlightId,
    permit: EstablishmentPermit,
    io: BoxConn,
    connected: Connected,
) {
    let mut completion = FlightCompletionGuard::new(context.cell.clone(), flight);
    let id = match next_connection_id(&context.pool) {
        Ok(id) => id,
        Err(error) => {
            completion.fail(Box::new(error));
            return;
        }
    };
    let info = ConnectionInfo::new(
        id,
        context.cell.id().origin().clone(),
        context.partition.id(),
        NegotiatedProtocol::Http2,
        connected,
    );
    let (connection, physical) = ConnectionState::establishing(info);
    let io = ConnectionIo::new(io, physical);
    let executor = PartitionExecutor {
        spawner: context.owner_spawner.clone(),
    };
    let (sender, driver) = match hyper::client::conn::http2::Builder::new(executor)
        .handshake::<_, SdkBody>(io)
        .await
    {
        Ok(established) => established,
        Err(error) => {
            connection.logical_close(CloseReason::ProtocolClosed);
            completion.fail(Box::new(error));
            return;
        }
    };

    if let Err(lease) = connection.open(permit.into_lease()) {
        drop(lease);
        connection.logical_close(CloseReason::ProtocolClosed);
        completion.fail(Box::new(std::io::Error::other(
            "HTTP/2 connection closed before installation",
        )));
        return;
    }

    let installed = OriginCell::complete_h2_flight(
        &context.cell,
        flight,
        connection.clone(),
        H2Sender::from_hyper(sender),
        context.cell.idle_deadline(),
    );
    let installed = match installed {
        Ok(installed) => installed,
        Err((_connection, _sender)) => {
            connection.logical_close(CloseReason::ProtocolClosed);
            completion.fail(Box::new(std::io::Error::other(
                "HTTP/2 flight became stale before installation",
            )));
            return;
        }
    };

    let generation = installed;
    let driver_guard = H2DriverGuard::new(H2CloseHandle::new(&context.cell, generation));
    let driver_info = connection.info().clone();
    context.owner_spawner.spawn(Box::pin(async move {
        if let Err(error) = driver.await {
            tracing::debug!(
                connection_id = %driver_info.id(),
                connection_partition = ?driver_info.owner_partition(),
                origin_scheme = %driver_info.origin().scheme(),
                origin_host = driver_info.origin().host(),
                origin_port = ?driver_info.origin().port(),
                error = ?error,
                "HTTP/2 connection driver failed"
            );
        }
        driver_guard.protocol_closed();
    }));

    completion.disarm();
    tracing::debug!(
        connection_id = %connection.id(),
        request_partition = ?context.partition.id(),
        connection_partition = ?connection.owner_partition(),
        origin_scheme = %connection.info().origin().scheme(),
        origin_host = connection.info().origin().host(),
        origin_port = ?connection.info().origin().port(),
        h2_generation = ?generation,
        "HTTP/2 connection established"
    );
}

/// Cloneable wrapper that preserves one flight failure for every participant.
#[derive(Clone, Debug)]
struct SharedFlightFailure(StdArc<BoxError>);

impl std::fmt::Display for SharedFlightFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

impl std::error::Error for SharedFlightFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref().as_ref())
    }
}

/// Fails every participant still retained by one exact flight.
fn fail_participants(cell: &OriginCell, flight: H2FlightId, error: StdArc<BoxError>) {
    let Some(participants) = cell.fail_h2_flight(flight) else {
        return;
    };
    for participant in participants {
        cell.complete_establishment(
            participant,
            AcquisitionResult::Failed(ConnectorError::other(
                Box::new(SharedFlightFailure(error.clone())),
                None,
            )),
        );
    }
}

#[cfg(all(test, not(smithy_http_client_loom)))]
mod tests {
    use super::*;
    use crate::client::pool::admission::ProtocolRequirement;
    use crate::client::pool::cell::AcquisitionEvent;
    use crate::client::pool::origin::OriginKey;
    use crate::client::pool::partition::EligibilityGroup;
    use http_1x::uri::Scheme;
    use std::error::Error as _;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll, Waker};

    fn cell() -> crate::sync::Arc<OriginCell> {
        crate::sync::Arc::new(OriginCell::new(
            super::super::super::partition::PartitionId::from_index(1),
            OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
            EligibilityGroup::Pool,
            None,
            None,
        ))
    }

    fn launching_waiter(cell: &crate::sync::Arc<OriginCell>) -> WaiterId {
        let waiter = OriginCell::register_waiter(cell, ProtocolRequirement::H2Required);
        let event = cell.poll_waiter(waiter, &mut Context::from_waker(Waker::noop()));
        let Poll::Ready(AcquisitionEvent::Establish(permit)) = event else {
            panic!("new H2 waiter did not receive establishment authority");
        };
        assert!(cell.start_establishment(waiter));
        drop(permit);
        waiter
    }

    fn failed_event(cell: &OriginCell, waiter: WaiterId) -> ConnectorError {
        let event = cell.poll_waiter(waiter, &mut Context::from_waker(Waker::noop()));
        let Poll::Ready(AcquisitionEvent::Complete(AcquisitionResult::Failed(error))) = event
        else {
            panic!("flight participant did not receive a failure");
        };
        error
    }

    #[test]
    fn flight_failure_reaches_every_live_participant() {
        let cell = cell();
        let first = launching_waiter(&cell);
        let second = launching_waiter(&cell);
        let H2FlightInstall::Driver(flight) = cell.install_or_join_h2_flight(first) else {
            panic!("first participant did not become the flight driver");
        };
        assert!(matches!(
            cell.install_or_join_h2_flight(second),
            H2FlightInstall::Joined
        ));

        let mut completion = FlightCompletionGuard::new(cell.clone(), flight);
        completion.fail(Box::new(std::io::Error::other(
            "synthetic HTTP/2 handshake failure",
        )));

        for waiter in [first, second] {
            let error = failed_event(&cell, waiter);
            assert!(error.is_other());
            let source = error.source().expect("flight failure lost its source");
            assert_eq!("synthetic HTTP/2 handshake failure", source.to_string());
            assert!(
                source
                    .source()
                    .expect("flight failure lost its original source")
                    .downcast_ref::<std::io::Error>()
                    .is_some(),
                "flight failure did not preserve the original error"
            );
        }
    }

    #[test]
    fn task_drop_fails_only_participants_still_owned_by_the_flight() {
        let cell = cell();
        let live = launching_waiter(&cell);
        let cancelled = launching_waiter(&cell);
        let H2FlightInstall::Driver(flight) = cell.install_or_join_h2_flight(live) else {
            panic!("first participant did not become the flight driver");
        };
        assert!(matches!(
            cell.install_or_join_h2_flight(cancelled),
            H2FlightInstall::Joined
        ));
        assert!(OriginCell::cancel_waiter(&cell, cancelled));

        drop(FlightCompletionGuard::new(cell.clone(), flight));

        let error = failed_event(&cell, live);
        assert!(error.is_other());
        assert_eq!(
            "HTTP/2 establishment task was dropped",
            error
                .source()
                .expect("task-drop failure lost its source")
                .to_string()
        );
        let successor = OriginCell::register_waiter(&cell, ProtocolRequirement::H2Required);
        assert_ne!(cancelled, successor);
        assert!(OriginCell::cancel_waiter(&cell, successor));
    }

    #[derive(Debug)]
    struct CountingSpawner {
        submissions: StdArc<AtomicUsize>,
    }

    impl DriverSpawner for CountingSpawner {
        fn spawn(&self, future: std::pin::Pin<Box<dyn Future<Output = ()> + Send + 'static>>) {
            self.submissions.fetch_add(1, Ordering::Relaxed);
            drop(future);
        }
    }

    #[test]
    fn hyper_executor_submits_work_to_the_partition_spawner() {
        let submissions = StdArc::new(AtomicUsize::new(0));
        let executor = PartitionExecutor {
            spawner: StdArc::new(CountingSpawner {
                submissions: submissions.clone(),
            }),
        };

        executor.execute(async {});

        assert_eq!(1, submissions.load(Ordering::Relaxed));
    }
}
