/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Partition-owned idle connection maintenance.
//!
//! Request and return paths only publish a revision after changing an idle
//! deadline. One task per partition snapshots the retained cells, closes
//! expired records, and sleeps until either the nearest remaining deadline or
//! another revision. The task retains this scheduler and its cells weakly so
//! it cannot keep a pool alive.

use super::cell::{CellId, OriginCell};
use super::partition::DriverSpawner;
use crate::sync::{Arc, Mutex, Weak};
use aws_smithy_async::rt::sleep::{AsyncSleep, SharedAsyncSleep};
use aws_smithy_async::time::SharedTimeSource;
use std::collections::HashMap;
use std::future::{poll_fn, Future};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, SystemTime};

/// Immutable pool policy copied into each partition scheduler.
#[derive(Clone, Debug, Default)]
pub(super) struct MaintenanceConfig {
    /// Idle duration applied to reusable H1 records.
    pub(super) idle_timeout: Option<Duration>,
    /// Clock used to assign and evaluate deadlines.
    pub(super) time_source: SharedTimeSource,
    /// Sleeper used by partition tasks.
    pub(super) sleep: Option<SharedAsyncSleep>,
}

/// Clock and wakeup state shared by one partition's maintenance task.
#[derive(Debug)]
pub(super) struct PartitionMaintenance {
    /// Idle duration applied whenever an H1 sender enters reusable storage.
    idle_timeout: Option<Duration>,
    /// Pool-owned clock used to assign and evaluate idle deadlines.
    time_source: SharedTimeSource,
    /// Pool-owned sleeper used by the partition task.
    sleep: Option<SharedAsyncSleep>,
    /// Registered cells, task state, and wake revision.
    state: Mutex<MaintenanceState>,
}

impl PartitionMaintenance {
    /// Creates a scheduler from immutable pool maintenance policy.
    pub(super) fn new(config: MaintenanceConfig) -> Arc<Self> {
        Arc::new(Self {
            idle_timeout: config.idle_timeout,
            time_source: config.time_source,
            sleep: config.sleep,
            state: Mutex::new(MaintenanceState::default()),
        })
    }

    /// Returns the deadline for a sender becoming idle now.
    pub(super) fn idle_deadline(&self) -> Option<SystemTime> {
        self.idle_timeout
            .and_then(|timeout| self.time_source.now().checked_add(timeout))
    }

    /// Registers a retained cell and wakes a task that may have no deadline.
    pub(super) fn register(&self, cell: &Arc<OriginCell>) {
        // Loom has no weak Arc, so the synchronization facade's modeled Weak
        // is strong. Skip this production-only index to avoid a model-only
        // maintenance -> cell -> maintenance cycle.
        #[cfg(all(test, smithy_http_client_loom))]
        {
            let _ = cell;
            self.notify();
        }

        #[cfg(not(all(test, smithy_http_client_loom)))]
        {
            let mut state = self.state.lock();
            state.cells.insert(cell.id().clone(), Weak::from_arc(cell));
            state.signal();
        }
    }

    /// Announces that the partition's nearest idle deadline may have changed.
    pub(super) fn notify(&self) {
        self.state.lock().signal();
    }

    /// Starts this partition's maintenance task at most once.
    pub(super) fn start(this: &Arc<Self>, spawner: &dyn DriverSpawner) {
        if this.idle_timeout.is_none() {
            return;
        }
        {
            let mut state = this.state.lock();
            if state.started {
                return;
            }
            state.started = true;
        }

        let scheduler = Weak::from_arc(this);
        spawner.spawn(Box::pin(async move {
            run(scheduler).await;
        }));
    }

    /// Captures the current revision without retaining the state lock.
    fn revision(&self) -> u64 {
        self.state.lock().revision
    }

    /// Returns live registered cells and prunes cells whose pool was dropped.
    fn cells(&self) -> Vec<Arc<OriginCell>> {
        let mut state = self.state.lock();
        let cells = state
            .cells
            .values()
            .filter_map(Weak::upgrade)
            .collect::<Vec<_>>();
        state.cells.retain(|_, cell| cell.upgrade().is_some());
        cells
    }

    /// Polls until a deadline publication changes `observed`.
    fn poll_revision(&self, observed: u64, cx: &Context<'_>) -> Poll<()> {
        let mut state = self.state.lock();
        if state.revision != observed {
            return Poll::Ready(());
        }
        if state
            .waker
            .as_ref()
            .is_none_or(|registered| !registered.will_wake(cx.waker()))
        {
            state.waker = Some(cx.waker().clone());
        }
        Poll::Pending
    }
}

/// Mutable scheduler state protected independently from every origin cell.
#[derive(Debug, Default)]
struct MaintenanceState {
    /// Retained partition cells indexed for replacement on duplicate publish.
    cells: HashMap<CellId, Weak<OriginCell>>,
    /// Monotonic change counter observed by the maintenance future.
    revision: u64,
    /// Task waiting for a new revision.
    waker: Option<Waker>,
    /// Whether the partition task was submitted to its owner spawner.
    started: bool,
}

impl MaintenanceState {
    /// Advances the wake revision and notifies the current task.
    fn signal(&mut self) {
        self.revision = self
            .revision
            .checked_add(1)
            .expect("partition maintenance revision exhausted");
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

/// Runs one scan per deadline or state change without retaining the scheduler.
async fn run(scheduler: Weak<PartitionMaintenance>) {
    let mut expiration_floor = None;
    loop {
        let Some(current) = scheduler.upgrade() else {
            return;
        };
        let observed = current.revision();
        let now = expiration_floor.take().map_or_else(
            || current.time_source.now(),
            |floor| current.time_source.now().max(floor),
        );
        let cells = current.cells();
        drop(current);

        for cell in &cells {
            OriginCell::expire_idle(cell, now);
        }
        let nearest = cells
            .iter()
            .filter_map(|cell| cell.nearest_idle_deadline())
            .min();

        let Some(current) = scheduler.upgrade() else {
            return;
        };
        let sleep = current.sleep.clone();
        drop(current);

        match (nearest, sleep) {
            (Some(deadline), Some(sleep)) => {
                let now = scheduler
                    .upgrade()
                    .map(|current| current.time_source.now())
                    .unwrap_or(deadline);
                let duration = deadline.duration_since(now).unwrap_or(Duration::ZERO);
                if wait_for_sleep_or_revision(&scheduler, observed, sleep, duration).await
                    == MaintenanceWake::DeadlineElapsed
                {
                    expiration_floor = Some(deadline);
                }
            }
            _ => wait_for_revision(&scheduler, observed).await,
        }
    }
}

/// Waits for a scheduler publication when no idle deadline exists.
async fn wait_for_revision(scheduler: &Weak<PartitionMaintenance>, observed: u64) {
    poll_fn(|cx| {
        scheduler.upgrade().map_or(Poll::Ready(()), |current| {
            current.poll_revision(observed, cx)
        })
    })
    .await
}

/// The event that ended a partition maintenance wait.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MaintenanceWake {
    /// The sleep for the nearest observed deadline completed.
    DeadlineElapsed,
    /// Idle residence changed or the scheduler was dropped.
    Revision,
}

/// Races one pool-provided sleep with a scheduler publication.
async fn wait_for_sleep_or_revision(
    scheduler: &Weak<PartitionMaintenance>,
    observed: u64,
    sleep: SharedAsyncSleep,
    duration: Duration,
) -> MaintenanceWake {
    let mut sleeping = Box::pin(sleep.sleep(duration));
    poll_fn(|cx| {
        if sleeping.as_mut().poll(cx).is_ready() {
            return Poll::Ready(MaintenanceWake::DeadlineElapsed);
        }
        scheduler
            .upgrade()
            .map_or(Poll::Ready(MaintenanceWake::Revision), |current| {
                current
                    .poll_revision(observed, cx)
                    .map(|()| MaintenanceWake::Revision)
            })
    })
    .await
}

#[cfg(all(test, feature = "rt-tokio", not(smithy_http_client_loom)))]
mod tests {
    use super::*;
    use crate::client::pool::cell::h1::H1Sender;
    use crate::client::pool::connection::{ConnectionInfo, ConnectionState, NegotiatedProtocol};
    use crate::client::pool::origin::OriginKey;
    use crate::client::pool::partition::{EligibilityGroup, PartitionId, TokioDriverSpawner};
    use aws_smithy_async::test_util::controlled_time_and_sleep;
    use aws_smithy_async::time::TimeSource;
    use aws_smithy_runtime_api::client::connection::ConnectionId;
    use http_1x::uri::Scheme;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, UNIX_EPOCH};

    #[derive(Clone, Debug)]
    struct RewindableTimeSource(Arc<AtomicU64>);

    impl RewindableTimeSource {
        fn new(seconds: u64) -> Self {
            Self(Arc::new(AtomicU64::new(seconds)))
        }

        fn set(&self, seconds: u64) {
            self.0.store(seconds, Ordering::SeqCst);
        }
    }

    impl TimeSource for RewindableTimeSource {
        fn now(&self) -> SystemTime {
            UNIX_EPOCH + Duration::from_secs(self.0.load(Ordering::SeqCst))
        }
    }

    fn cell_with_maintenance(
        timeout: Duration,
        time_source: SharedTimeSource,
        sleep: SharedAsyncSleep,
    ) -> (Arc<PartitionMaintenance>, Arc<OriginCell>) {
        let maintenance = PartitionMaintenance::new(MaintenanceConfig {
            idle_timeout: Some(timeout),
            time_source,
            sleep: Some(sleep),
        });
        let origin = OriginKey::from_parts(Scheme::HTTP, "example.com", None).unwrap();
        let cell = Arc::new(OriginCell::new(
            PartitionId::from_index(1),
            origin,
            EligibilityGroup::Pool,
            None,
            Some(maintenance.clone()),
        ));
        maintenance.register(&cell);
        (maintenance, cell)
    }

    fn managed_cell(
        timeout: Duration,
    ) -> (
        Arc<PartitionMaintenance>,
        Arc<OriginCell>,
        aws_smithy_async::test_util::SleepGate,
    ) {
        let (time, sleep, gate) = controlled_time_and_sleep(UNIX_EPOCH);
        let (maintenance, cell) = cell_with_maintenance(
            timeout,
            SharedTimeSource::new(time),
            SharedAsyncSleep::new(sleep),
        );
        (maintenance, cell, gate)
    }

    fn connection(id: u64) -> Arc<ConnectionState> {
        let info = ConnectionInfo::new(
            ConnectionId::new(id),
            OriginKey::from_parts(Scheme::HTTP, "example.com", None).unwrap(),
            PartitionId::from_index(1),
            NegotiatedProtocol::Http1,
            None,
            None,
            false,
        );
        let (connection, _physical) = ConnectionState::unbounded(info);
        connection
    }

    #[tokio::test]
    async fn idle_record_closes_at_its_fake_time_deadline() {
        let timeout = Duration::from_secs(10);
        let (maintenance, cell, mut gate) = managed_cell(timeout);
        let connection = connection(1);
        OriginCell::install_idle_h1(&cell, connection.clone(), H1Sender::test(1));
        PartitionMaintenance::start(&maintenance, &TokioDriverSpawner::current());

        let sleep = gate.expect_sleep().await;
        assert_eq!(timeout, sleep.duration());
        assert_eq!(None, connection.snapshot().close_reason);
        sleep.allow_progress();

        for _ in 0..10 {
            if connection.snapshot().close_reason == Some(super::super::CloseReason::IdleTimeout) {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(
            Some(super::super::CloseReason::IdleTimeout),
            connection.snapshot().close_reason
        );
    }

    #[tokio::test]
    async fn selected_record_has_no_idle_deadline() {
        let timeout = Duration::from_secs(10);
        let (maintenance, cell, mut gate) = managed_cell(timeout);
        let connection = connection(1);
        OriginCell::install_idle_h1(&cell, connection.clone(), H1Sender::test(1));
        PartitionMaintenance::start(&maintenance, &TokioDriverSpawner::current());

        let sleep = gate.expect_sleep().await;
        let selection = OriginCell::select_h1(&cell).expect("idle H1 was not selected");
        sleep.allow_progress();
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }

        assert_eq!(None, connection.snapshot().close_reason);
        drop(selection);
    }

    #[tokio::test]
    async fn completed_sleep_is_an_expiration_floor_after_clock_moves_backward() {
        let timeout = Duration::from_secs(10);
        let clock = RewindableTimeSource::new(100);
        let (_unused_time, sleep, mut gate) = controlled_time_and_sleep(UNIX_EPOCH);
        let (maintenance, cell) = cell_with_maintenance(
            timeout,
            SharedTimeSource::new(clock.clone()),
            SharedAsyncSleep::new(sleep),
        );
        let connection = connection(1);
        OriginCell::install_idle_h1(&cell, connection.clone(), H1Sender::test(1));
        PartitionMaintenance::start(&maintenance, &TokioDriverSpawner::current());

        let sleep = gate.expect_sleep().await;
        assert_eq!(timeout, sleep.duration());
        clock.set(90);
        sleep.allow_progress();

        for _ in 0..10 {
            if connection.snapshot().close_reason == Some(super::super::CloseReason::IdleTimeout) {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(
            Some(super::super::CloseReason::IdleTimeout),
            connection.snapshot().close_reason
        );
    }
}
