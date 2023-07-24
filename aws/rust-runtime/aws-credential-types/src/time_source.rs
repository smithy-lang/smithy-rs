/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_async::time::TimeSource as TimeSourceTrait;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

impl TimeSourceTrait for TestingTimeSource {
    fn now(&self) -> SystemTime {
        self.now()
    }
}

/// Time Source that can be manually moved for tests
/// > This has been superseded by [`aws_smithy_async::time::TimeSource`] and will be removed in a
/// > future release.
///
/// # Examples
///
/// use aws_credential_types::time_source::{TestingTimeSource, TimeSource};
/// use std::time::{UNIX_EPOCH, Duration};
/// let mut time = TestingTimeSource::new(UNIX_EPOCH);
/// let client = Client::with_timesource(TimeSource::testing(&time));
/// time.advance(Duration::from_secs(100));
/// ```
#[derive(Clone, Debug)]
pub(crate) struct TestingTimeSource {
    queries: Arc<Mutex<Vec<SystemTime>>>,
    now: Arc<Mutex<SystemTime>>,
}

impl TestingTimeSource {
    /// Creates `TestingTimeSource` with `start_time`.
    pub(crate) fn new(start_time: SystemTime) -> Self {
        Self {
            queries: Default::default(),
            now: Arc::new(Mutex::new(start_time)),
        }
    }

    /// Sets time to the specified `time`.
    pub(crate) fn set_time(&mut self, time: SystemTime) {
        let mut now = self.now.lock().unwrap();
        *now = time;
    }

    /// Advances time by `delta`.
    pub(crate) fn advance(&mut self, delta: Duration) {
        let mut now = self.now.lock().unwrap();
        *now += delta;
    }

    /// Returns the current time understood by `TestingTimeSource`.
    pub(crate) fn now(&self) -> SystemTime {
        let ts = *self.now.lock().unwrap();
        self.queries.lock().unwrap().push(ts);
        ts
    }
}

#[cfg(test)]
mod test {
    use super::TestingTimeSource;

    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn testing_time_source_should_behave_as_expected() {
        let mut time_source = TestingTimeSource::new(UNIX_EPOCH);
        assert_eq!(time_source.now(), UNIX_EPOCH);
        time_source.advance(Duration::from_secs(10));
        assert_eq!(time_source.now(), UNIX_EPOCH + Duration::from_secs(10));
    }
}
