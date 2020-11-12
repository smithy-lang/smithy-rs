/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use chrono::{DateTime, NaiveDateTime, SecondsFormat, Utc};
use std::time::{Duration, SystemTime, SystemTimeError, UNIX_EPOCH};

// TODO: fork HTTP date to expose internals, consider replacing Chrono depending on parse difficulty...

#[derive(Debug, PartialEq, Clone)]
pub struct Instant {
    seconds: i64,
    nanos: u32,
}

pub mod instant {
    pub enum Format {
        DateTime,
        HttpDate,
        EpochSeconds,
    }
}

impl Instant {
    pub fn from_epoch_seconds(epoch_seconds: i64) -> Self {
        Instant {
            seconds: epoch_seconds,
            nanos: 0,
        }
    }

    pub fn from_fractional_seconds(epoch_seconds: i64, fraction: f64) -> Self {
        Instant {
            seconds: epoch_seconds,
            nanos: (fraction * 1_000_000_000_f64) as u32,
        }
    }

    pub fn from_system_time(system_time: SystemTime) -> Result<Self, SystemTimeError> {
        let duration = system_time.duration_since(UNIX_EPOCH)?;
        Ok(Instant {
            seconds: duration.as_secs() as i64,
            nanos: duration.subsec_nanos(),
        })
    }

    fn to_chrono(&self) -> DateTime<Utc> {
        DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(self.seconds, self.nanos), Utc)
    }

    pub fn fmt(&self, format: instant::Format) -> String {
        match format {
            instant::Format::DateTime => {
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
            instant::Format::EpochSeconds => {
                if self.nanos == 0 {
                    format!("{}", self.seconds)
                } else {
                    let fraction = format!("{:0>9}", self.nanos);
                    format!("{}.{}", self.seconds, fraction.trim_end_matches('0'))
                }
            }
            instant::Format::HttpDate => {
                assert!(self.seconds >= 0);
                let system_time: SystemTime = UNIX_EPOCH + Duration::from_secs(self.seconds as u64);
                httpdate::fmt_http_date(system_time)
            }
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct Blob {
    inner: Vec<u8>,
}

impl Blob {
    pub fn new<T: Into<Vec<u8>>>(inp: T) -> Self {
        Blob { inner: inp.into() }
    }
}

impl AsRef<[u8]> for Blob {
    fn as_ref(&self) -> &[u8] {
        &self.inner
    }
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
            "Mon, 16 Dec 2019 23:48:18 GMT"
        );
    }
}
