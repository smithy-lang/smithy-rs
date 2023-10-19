/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::collections::VecDeque;
use std::fmt;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Copy)]
pub(super) struct Throughput {
    bytes_read: f64,
    per_time_elapsed: Duration,
}

impl Throughput {
    pub(super) fn new(bytes_read: f64, per_time_elapsed: Duration) -> Self {
        debug_assert!(
            !bytes_read.is_nan(),
            "cannot create a throughput if bytes_read == NaN"
        );
        debug_assert!(
            bytes_read.is_finite(),
            "cannot create a throughput if bytes_read == Inf"
        );
        debug_assert!(
            !per_time_elapsed.is_zero(),
            "cannot create a throughput if per_time_elapsed == 0"
        );

        Self {
            bytes_read,
            per_time_elapsed,
        }
    }

    pub(super) fn per_time_elapsed(&self) -> Duration {
        self.per_time_elapsed
    }

    pub(super) fn bytes_per_second(&self) -> f64 {
        let per_time_elapsed_secs = self.per_time_elapsed.as_secs_f64();
        if per_time_elapsed_secs == 0.0 {
            return 0.0; // Avoid dividing by zero.
        };

        self.bytes_read / per_time_elapsed_secs
    }

    fn normalize_to_per_one_second(self) -> Self {
        if self.per_time_elapsed == Duration::from_secs(1) {
            // Already normalized to per second, return self without modifying it.
            self
        } else {
            // Normalize to per second and return the result.
            Self {
                bytes_read: self.bytes_per_second(),
                per_time_elapsed: Duration::from_secs(1),
            }
        }
    }
}

impl PartialEq for Throughput {
    fn eq(&self, other: &Self) -> bool {
        if self.per_time_elapsed == other.per_time_elapsed {
            // If both are using the same unit of time, compare bytes per second.
            self.bytes_per_second() == other.bytes_per_second()
        } else {
            // Otherwise, normalize the unit of time and then compare adjusted bytes per second.
            let normalized_self = self.normalize_to_per_one_second();
            let normalized_other = other.normalize_to_per_one_second();
            normalized_self.bytes_per_second() == normalized_other.bytes_per_second()
        }
    }
}

impl PartialOrd for Throughput {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        if self.per_time_elapsed == other.per_time_elapsed {
            // If both are using the same unit of time, compare bytes per second.
            self.bytes_per_second()
                .partial_cmp(&other.bytes_per_second())
        } else {
            // Otherwise, normalize the unit of time and then compare adjusted bytes per second.
            let normalized_self = self.normalize_to_per_one_second();
            let normalized_other = other.normalize_to_per_one_second();
            normalized_self
                .bytes_per_second()
                .partial_cmp(&normalized_other.bytes_per_second())
        }
    }
}

impl fmt::Display for Throughput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The default float formatting behavior will ensure the a number like 2.000 is rendered as 2
        // while a number like 0.9982107441748642 will be rendered as 0.9982107441748642. This
        // multiplication and division will truncate a float to have a precision of no greater than 3.
        // For example, 0.9982107441748642 would become 0.999. This will fail for very large floats
        // but should suffice for the numbers we're dealing with.
        let pretty_bytes_per_second = (self.bytes_per_second() * 1000.0).round() / 1000.0;

        write!(f, "{pretty_bytes_per_second} B/s")
    }
}

impl From<(u64, Duration)> for Throughput {
    fn from(value: (u64, Duration)) -> Self {
        Self {
            bytes_read: value.0 as f64,
            per_time_elapsed: value.1,
        }
    }
}

#[derive(Clone)]
pub(super) struct ThroughputLogs {
    max_length: usize,
    min_elapsed_time: Duration,
    inner: VecDeque<(SystemTime, u64)>,
    floating_back: Option<(SystemTime, u64)>,
}

impl ThroughputLogs {
    pub(super) fn new(max_length: usize, min_elapsed_time: Duration) -> Self {
        Self {
            inner: VecDeque::new(),
            min_elapsed_time,
            max_length,
            floating_back: None,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub(super) fn push(&mut self, throughput: (SystemTime, u64)) {
        // Clear the floating back since we now have a real one.
        let _ = self.floating_back.take();
        self.inner.push_back(throughput);

        // When the number of logs exceeds the max length, toss the oldest log.
        if self.inner.len() > self.max_length {
            self.inner.pop_front();
        }
    }

    pub(super) fn set_floating_back(&mut self, time: SystemTime) {
        self.floating_back = Some((time, 0));
    }

    pub(super) fn front(&self) -> Option<&(SystemTime, u64)> {
        self.inner.front()
    }

    // Call `self.inner.back()` instead of this and you'll be confused and then sorry.
    pub(super) fn back(&self) -> Option<&(SystemTime, u64)> {
        self.floating_back.as_ref().or(self.inner.back())
    }

    pub(super) fn calculate_throughput(&self) -> Result<Option<Throughput>, super::Error> {
        match (self.front(), self.back()) {
            (Some((front_t, _)), Some((back_t, _))) => {
                // Ensure that enough time has passed between the first and last logs.
                // If not, we can't calculate throughput.
                let log_time_span = back_t
                    .duration_since(*front_t)
                    .map_err(|err| super::Error::TimeTravel(err.into()))?;
                if log_time_span < self.min_elapsed_time {
                    return Ok(None);
                }

                // Floating back never contains bytes, so we don't care that
                // it's missed in this calculation.
                let total_bytes_logged = self
                    .inner
                    .iter()
                    .fold(0, |acc, (_, bytes_read)| acc + bytes_read)
                    as f64;

                Ok(Some(Throughput {
                    bytes_read: total_bytes_logged,
                    per_time_elapsed: log_time_span,
                }))
            }
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod test {
    use super::{Throughput, ThroughputLogs};
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn test_throughput_eq() {
        let t1 = Throughput::new(1.0, Duration::from_secs(1));
        let t2 = Throughput::new(25.0, Duration::from_secs(25));
        let t3 = Throughput::new(100.0, Duration::from_secs(100));

        assert_eq!(t1, t2);
        assert_eq!(t2, t3);
    }

    fn build_throughput_log(length: u32, tick_duration: Duration, rate: u64) -> ThroughputLogs {
        let mut throughput_logs = ThroughputLogs::new(length as usize, Duration::from_secs(1));
        for i in 1..=length {
            throughput_logs.push((UNIX_EPOCH + (tick_duration * i), rate));
        }

        assert_eq!(length as usize, throughput_logs.inner.len());
        throughput_logs
    }

    #[test]
    fn test_throughput_log_calculate_throughput_1() {
        let throughput_logs = build_throughput_log(1000, Duration::from_secs(1), 1);

        let throughput = throughput_logs.calculate_throughput().unwrap().unwrap();
        // Floats being what they are
        assert_eq!(1.001001001001001, throughput.bytes_per_second());
    }

    #[test]
    fn test_throughput_log_calculate_throughput_2() {
        let throughput_logs = build_throughput_log(1000, Duration::from_secs(5), 5);

        let throughput = throughput_logs.calculate_throughput().unwrap().unwrap();
        // Floats being what they are
        assert_eq!(1.001001001001001, throughput.bytes_per_second());
    }

    #[test]
    fn test_throughput_log_calculate_throughput_3() {
        let throughput_logs = build_throughput_log(1000, Duration::from_millis(200), 1024);

        let throughput = throughput_logs.calculate_throughput().unwrap().unwrap();
        let expected_throughput = 1024.0 * 5.0;
        // Floats being what they are
        assert_eq!(
            expected_throughput + 5.125125125125,
            throughput.bytes_per_second()
        );
    }

    #[test]
    fn test_throughput_log_calculate_throughput_4() {
        let throughput_logs = build_throughput_log(1000, Duration::from_millis(100), 12);

        let throughput = throughput_logs.calculate_throughput().unwrap().unwrap();
        let expected_throughput = 12.0 * 10.0;

        // Floats being what they are
        assert_eq!(
            expected_throughput + 0.12012012012012,
            throughput.bytes_per_second()
        );
    }
}