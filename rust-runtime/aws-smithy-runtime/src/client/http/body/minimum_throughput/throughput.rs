/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::fmt;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub(super) struct Throughput {
    pub(super) bytes_read: f64,
    pub(super) per_time_elapsed: Duration,
}

impl Throughput {
    pub(super) fn bytes_per_second(&self) -> f64 {
        self.bytes_read / self.per_time_elapsed.as_secs_f64()
    }

    fn normalize_to_per_second(self) -> Self {
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
            let normalized_self = self.normalize_to_per_second();
            let normalized_other = other.normalize_to_per_second();
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
            let normalized_self = self.normalize_to_per_second();
            let normalized_other = other.normalize_to_per_second();
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
