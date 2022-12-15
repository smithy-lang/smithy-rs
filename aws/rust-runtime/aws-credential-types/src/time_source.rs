/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::ops::Deref;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// Time source abstraction
///
/// Simple abstraction representing time either real-time or manually-specified
///
/// # Examples
///
/// ```rust
/// # struct Client {
/// #  // stub
/// # }
/// #
/// # impl Client {
/// #     fn with_timesource(ts: TimeSource) -> Self {
/// #         Client { }
/// #     }
/// # }
/// use aws_credential_types::TimeSource;
/// let time = TimeSource::real();
/// let client = Client::with_timesource(time);
/// ```
#[derive(Debug, Clone)]
pub struct TimeSource(Inner);

impl TimeSource {
    /// Creates `TimeSource` from the current system time.
    pub fn real() -> Self {
        TimeSource(Inner::Real)
    }

    /// Creates `TimeSource` from the manually specified `time_source`.
    pub fn manual(time_source: &ManualTimeSource) -> Self {
        TimeSource(Inner::Manual(time_source.clone()))
    }

    /// Returns the current system time based on the mode.
    pub fn now(&self) -> SystemTime {
        match &self.0 {
            Inner::Real => SystemTime::now(),
            Inner::Manual(manual) => manual.now(),
        }
    }
}

impl Default for TimeSource {
    fn default() -> Self {
        TimeSource::real()
    }
}

/// Time Source that can be manually moved for tests
///
/// # Examples
///
/// ```rust
/// # struct Client {
/// #  // stub
/// # }
/// #
/// # impl Client {
/// #     fn with_timesource(ts: TimeSource) -> Self {
/// #         Client { }
/// #     }
/// # }
/// use aws_credential_types::{ManualTimeSource, TimeSource};
/// use std::time::{UNIX_EPOCH, Duration};
/// let mut time = ManualTimeSource::new(UNIX_EPOCH);
/// let client = Client::with_timesource(TimeSource::manual(&time));
/// time.advance(Duration::from_secs(100));
/// ```
#[derive(Clone, Debug)]
pub struct ManualTimeSource {
    queries: Arc<Mutex<Vec<SystemTime>>>,
    now: Arc<Mutex<SystemTime>>,
}

impl ManualTimeSource {
    /// Creates `ManualTimeSource` with `start_time`.
    pub fn new(start_time: SystemTime) -> Self {
        Self {
            queries: Default::default(),
            now: Arc::new(Mutex::new(start_time)),
        }
    }

    /// Sets time to the specified `time`.
    pub fn set_time(&mut self, time: SystemTime) {
        let mut now = self.now.lock().unwrap();
        *now = time;
    }

    /// Advances time by `delta`.
    pub fn advance(&mut self, delta: Duration) {
        let mut now = self.now.lock().unwrap();
        *now += delta;
    }

    /// Returns a `Vec` of queried times so far.
    pub fn queries(&self) -> impl Deref<Target = Vec<SystemTime>> + '_ {
        self.queries.lock().unwrap()
    }

    /// Returns the current time understood by `ManualTimeSource`.
    pub fn now(&self) -> SystemTime {
        let ts = *self.now.lock().unwrap();
        self.queries.lock().unwrap().push(ts);
        ts
    }
}

// In the future, if needed we can add a time source trait, however, the manual time source
// should cover most test use cases.
#[derive(Debug, Clone)]
enum Inner {
    Real,
    Manual(ManualTimeSource),
}

#[cfg(test)]
mod test {
    use std::time::{Duration, UNIX_EPOCH};

    use crate::{ManualTimeSource, TimeSource};

    #[test]
    fn ts_works() {
        let real = TimeSource::real();
        // no panics
        let _ = real.now();

        let mut manual = ManualTimeSource::new(UNIX_EPOCH);
        let ts = TimeSource::manual(&manual);
        assert_eq!(ts.now(), UNIX_EPOCH);
        manual.advance(Duration::from_secs(10));
        assert_eq!(ts.now(), UNIX_EPOCH + Duration::from_secs(10));
    }
}
