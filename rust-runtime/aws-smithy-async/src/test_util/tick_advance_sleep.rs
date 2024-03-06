/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Test time/sleep implementations that work by manually advancing time with a `tick()`
//!
//! # Examples
//!
//! Spawning a task that creates new sleep tasks and waits for them sequentially,
//! and advancing passed all of them with a single call to `tick()`.
//!
//! ```rust,no_run
//! use std::time::{Duration, SystemTime};
//! use aws_smithy_async::test_util::tick_advance_sleep::tick_advance_time_and_sleep;
//! use aws_smithy_async::time::TimeSource;
//! use aws_smithy_async::rt::sleep::AsyncSleep;
//!
//! # async fn example() {
//! // Create the test time/sleep implementations.
//! // They will start at SystemTime::UNIX_EPOCH.
//! let (time, sleep) = tick_advance_time_and_sleep();
//!
//! // Spawn the task that sequentially sleeps
//! let task = tokio::spawn(async move {
//!     sleep.sleep(Duration::from_secs(1)).await;
//!     sleep.sleep(Duration::from_secs(2)).await;
//!     sleep.sleep(Duration::from_secs(3)).await;
//! });
//! // Verify that task hasn't done anything yet since we haven't called `tick`
//! tokio::task::yield_now().await;
//! assert!(!task.is_finished());
//! assert_eq!(SystemTime::UNIX_EPOCH, time.now());
//!
//! // Tick 6 seconds, which is long enough to go passed all the sequential sleeps
//! time.tick(Duration::from_secs(6)).await;
//! assert_eq!(SystemTime::UNIX_EPOCH + Duration::from_secs(6), time.now());
//!
//! // Verify the task joins, indicating all the sleeps are done
//! task.await.unwrap();
//! # }
//! ```

use crate::{
    rt::sleep::{AsyncSleep, Sleep},
    time::TimeSource,
};
use std::{
    future::IntoFuture,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};
use tokio::sync::oneshot::Sender;

#[derive(Debug)]
struct QueuedSleep {
    presents_at: Duration,
    notify: Option<Sender<()>>,
}

#[derive(Default, Debug)]
struct Inner {
    sleeps: Vec<QueuedSleep>,
    now: Duration,
}

#[derive(Clone, Default, Debug)]
struct SharedInner {
    inner: Arc<Mutex<Inner>>,
}
impl SharedInner {
    fn get(&self) -> impl Deref<Target = Inner> + '_ {
        self.inner.lock().unwrap()
    }
    fn get_mut(&self) -> impl DerefMut<Target = Inner> + '_ {
        self.inner.lock().unwrap()
    }

    fn take_presenting_before(&self, time: Duration) -> Vec<QueuedSleep> {
        let mut inner = self.get_mut();

        // Tick to each individual sleep time and yield the runtime
        // so that any futures waiting on a sleep run before futures
        // waiting on a later sleep.
        inner.sleeps.sort_by_key(|s| s.presents_at);
        let partition_index = inner
            .sleeps
            .iter()
            .enumerate()
            .find(|(_, s)| s.presents_at > time)
            .map(|(i, _)| i);
        let sleep_count = inner.sleeps.len();
        let presenting: Vec<_> = inner
            .sleeps
            .drain(0..partition_index.unwrap_or(sleep_count))
            .collect();
        presenting
    }
}

/// Tick-advancing test sleep implementation.
///
/// See [module docs](crate::test_util::tick_advance_sleep) for more information.
#[derive(Clone, Debug)]
pub struct TickAdvanceSleep {
    inner: SharedInner,
}

impl AsyncSleep for TickAdvanceSleep {
    fn sleep(&self, duration: Duration) -> Sleep {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let mut inner = self.inner.get_mut();
        let now = inner.now;
        inner.sleeps.push(QueuedSleep {
            presents_at: now + duration,
            notify: Some(tx),
        });
        Sleep::new(async move {
            let _ = rx.into_future().await;
        })
    }
}

/// Tick-advancing test time source implementation.
///
/// See [module docs](crate::test_util::tick_advance_sleep) for more information.
#[derive(Clone, Debug)]
pub struct TickAdvanceTime {
    inner: SharedInner,
}

impl TickAdvanceTime {
    /// Advance time by `duration`.
    ///
    /// This will yield the async runtime after each sleep that presents between
    /// the previous current time and the time after the given duration. This allows
    /// for async tasks pending one of those sleeps to do some work and also create
    /// additional sleep tasks. Created sleep tasks may also complete during this
    /// call to `tick()` if they present before the given time duration.
    pub async fn tick(&self, duration: Duration) {
        let time = self.inner.get().now + duration;

        let mut presenting = self.inner.take_presenting_before(time);
        while !presenting.is_empty() {
            for mut sleep in presenting {
                self.inner.get_mut().now = sleep.presents_at;
                let _ = sleep.notify.take().unwrap().send(());
                tokio::task::yield_now().await;
            }

            // Unblocked tasks could have queued some more sleeps within the duration
            presenting = self.inner.take_presenting_before(time);
        }

        self.inner.get_mut().now = time;
    }
}

impl TimeSource for TickAdvanceTime {
    fn now(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + self.inner.get().now
    }
}

/// Creates tick-advancing test time/sleep implementations.
///
/// See [module docs](crate::test_util::tick_advance_sleep) for more information.
pub fn tick_advance_time_and_sleep() -> (TickAdvanceTime, TickAdvanceSleep) {
    let inner = SharedInner::default();
    (
        TickAdvanceTime {
            inner: inner.clone(),
        },
        TickAdvanceSleep {
            inner: inner.clone(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tick_advances() {
        let (time, sleep) = tick_advance_time_and_sleep();

        assert_eq!(SystemTime::UNIX_EPOCH, time.now());
        time.tick(Duration::from_secs(1)).await;
        assert_eq!(SystemTime::UNIX_EPOCH + Duration::from_secs(1), time.now());

        let sleeps = vec![
            tokio::spawn(sleep.sleep(Duration::from_millis(500))),
            tokio::spawn(sleep.sleep(Duration::from_secs(1))),
            tokio::spawn(sleep.sleep(Duration::from_secs(2))),
            tokio::spawn(sleep.sleep(Duration::from_secs(3))),
            tokio::spawn(sleep.sleep(Duration::from_secs(4))),
        ];

        tokio::task::yield_now().await;
        for sleep in &sleeps {
            assert!(!sleep.is_finished());
        }

        time.tick(Duration::from_secs(1)).await;
        assert_eq!(SystemTime::UNIX_EPOCH + Duration::from_secs(2), time.now());
        assert!(sleeps[0].is_finished());
        assert!(sleeps[1].is_finished());
        assert!(!sleeps[2].is_finished());
        assert!(!sleeps[3].is_finished());
        assert!(!sleeps[4].is_finished());

        time.tick(Duration::from_secs(2)).await;
        assert_eq!(SystemTime::UNIX_EPOCH + Duration::from_secs(4), time.now());
        assert!(sleeps[2].is_finished());
        assert!(sleeps[3].is_finished());
        assert!(!sleeps[4].is_finished());

        time.tick(Duration::from_secs(1)).await;
        assert_eq!(SystemTime::UNIX_EPOCH + Duration::from_secs(5), time.now());
        assert!(sleeps[4].is_finished());
    }

    #[tokio::test]
    async fn sleep_leading_to_sleep() {
        let (time, sleep) = tick_advance_time_and_sleep();

        let task = tokio::spawn(async move {
            sleep.sleep(Duration::from_secs(1)).await;
            sleep.sleep(Duration::from_secs(2)).await;
            sleep.sleep(Duration::from_secs(3)).await;
        });
        tokio::task::yield_now().await;
        assert!(!task.is_finished());
        assert_eq!(SystemTime::UNIX_EPOCH, time.now());

        time.tick(Duration::from_secs(6)).await;
        assert_eq!(SystemTime::UNIX_EPOCH + Duration::from_secs(6), time.now());
        task.await.unwrap();
    }
}
