/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use crate::instant::format::DateParseError;
use chrono::{DateTime, NaiveDateTime, SecondsFormat, Utc};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

mod format;

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Instant {
    seconds: i64,
    subsecond_nanos: u32,
}

impl Instant {
    pub fn from_epoch_seconds(epoch_seconds: i64) -> Self {
        Instant {
            seconds: epoch_seconds,
            subsecond_nanos: 0,
        }
    }

    pub fn from_fractional_seconds(epoch_seconds: i64, fraction: f64) -> Self {
        let subsecond_nanos = (fraction * 1_000_000_000_f64) as u32;
        Instant::from_secs_and_nanos(epoch_seconds, subsecond_nanos)
    }

    pub fn from_secs_and_nanos(seconds: i64, subsecond_nanos: u32) -> Self {
        if subsecond_nanos >= 1_000_000_000 {
            panic!("{} is > 1_000_000_000", subsecond_nanos)
        }
        Instant {
            seconds,
            subsecond_nanos,
        }
    }

    pub fn from_f64(epoch_seconds: f64) -> Self {
        let seconds = epoch_seconds.floor() as i64;
        let rem = epoch_seconds - epoch_seconds.floor();
        Instant::from_fractional_seconds(seconds, rem)
    }

    pub fn from_system_time(system_time: SystemTime) -> Self {
        let duration = system_time
            .duration_since(UNIX_EPOCH)
            .expect("SystemTime can never represent a time before the Unix Epoch");
        Instant {
            seconds: duration.as_secs() as i64,
            subsecond_nanos: duration.subsec_nanos(),
        }
    }

    pub fn from_str(s: &str, format: Format) -> Result<Self, DateParseError> {
        match format {
            Format::DateTime => format::iso_8601::parse(s),
            Format::HttpDate => format::http_date::parse(s),
            Format::EpochSeconds => <f64>::from_str(s)
                // TODO: Parse base & fraction separately to achieve higher precision
                .map(Self::from_f64)
                .map_err(|_| DateParseError::Invalid("expected float")),
        }
    }

    fn to_chrono(&self) -> DateTime<Utc> {
        DateTime::<Utc>::from_utc(
            NaiveDateTime::from_timestamp(self.seconds, self.subsecond_nanos),
            Utc,
        )
    }

    pub fn has_nanos(&self) -> bool {
        self.subsecond_nanos != 0
    }

    pub fn epoch_fractional_seconds(&self) -> f64 {
        self.seconds as f64 + self.subsecond_nanos as f64 / 1_000_000_000_f64
    }

    pub fn epoch_seconds(&self) -> i64 {
        self.seconds
    }

    pub fn fmt(&self, format: Format) -> String {
        match format {
            Format::DateTime => {
                // TODO: hand write rfc3339 formatter & remove Chrono alloc feature
                let rfc3339 = self
                    .to_chrono()
                    .to_rfc3339_opts(SecondsFormat::AutoSi, true);
                // There's a bug(?) where trailing 0s aren't trimmed
                let mut rfc3339 = rfc3339
                    .trim_end_matches('Z')
                    .trim_end_matches('0')
                    .to_owned();
                rfc3339.push('Z');
                rfc3339
            }
            Format::EpochSeconds => {
                if self.subsecond_nanos == 0 {
                    format!("{}", self.seconds)
                } else {
                    let fraction = format!("{:0>9}", self.subsecond_nanos);
                    format!("{}.{}", self.seconds, fraction.trim_end_matches('0'))
                }
            }
            Format::HttpDate => format::http_date::format(&self),
        }
    }
}

pub enum Format {
    DateTime,
    HttpDate,
    EpochSeconds,
}
#[cfg(test)]
mod test {
    use crate::instant::Format;
    use crate::Instant;

    #[test]
    fn test_instant_fmt() {
        let instant = Instant::from_epoch_seconds(1576540098);
        assert_eq!(instant.fmt(Format::DateTime), "2019-12-16T23:48:18Z");
        assert_eq!(instant.fmt(Format::EpochSeconds), "1576540098");
        assert_eq!(
            instant.fmt(Format::HttpDate),
            "Mon, 16 Dec 2019 23:48:18 GMT"
        );

        let instant = Instant::from_fractional_seconds(1576540098, 0.52);
        assert_eq!(instant.fmt(Format::DateTime), "2019-12-16T23:48:18.52Z");
        assert_eq!(instant.fmt(Format::EpochSeconds), "1576540098.52");
        assert_eq!(
            instant.fmt(Format::HttpDate),
            "Mon, 16 Dec 2019 23:48:18.520 GMT"
        );
    }
}
