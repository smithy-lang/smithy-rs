/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Instant value for representing Smithy timestamps.
//!
//! Unlike [`std::time::Instant`], this instant is not opaque. The time inside of it can be
//! read and modified. It also holds logic for parsing and formatting timestamps in any of
//! the timestamp formats that [Smithy](https://awslabs.github.io/smithy/) supports.

use self::format::InstantParseError;
use num_integer::div_mod_floor;
use num_integer::Integer;
use std::convert::TryFrom;
use std::error::Error as StdError;
use std::fmt;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

mod format;

const MILLIS_PER_SECOND: i64 = 1000;
const NANOS_PER_MILLI: u32 = 1_000_000;
const NANOS_PER_SECOND: i128 = 1_000_000_000;
const NANOS_PER_SECOND_U32: u32 = 1_000_000_000;

/* ANCHOR: instant */

/// Instant in time.
///
/// Instant in time represented as seconds and sub-second nanos since
/// the Unix epoch (January 1, 1970 at midnight UTC/GMT).
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Instant {
    seconds: i64,
    subsecond_nanos: u32,
}

/* ANCHOR_END: instant */

impl Instant {
    /// Creates an `Instant` from a number of seconds since the Unix epoch.
    pub fn from_secs(epoch_seconds: i64) -> Self {
        Instant {
            seconds: epoch_seconds,
            subsecond_nanos: 0,
        }
    }

    /// Converts number of milliseconds since the Unix epoch into an `Instant`.
    pub fn from_millis(epoch_millis: i64) -> Instant {
        let (seconds, millis) = div_mod_floor(epoch_millis, MILLIS_PER_SECOND);
        Instant::from_secs_and_nanos(seconds, millis as u32 * NANOS_PER_MILLI)
    }

    /// Creates an `Instant` from a number of nanoseconds since the Unix epoch.
    pub fn from_nanos(epoch_nanos: i128) -> Result<Self, ConversionError> {
        let (seconds, subsecond_nanos) = epoch_nanos.div_mod_floor(&NANOS_PER_SECOND);
        let seconds = i64::try_from(seconds).map_err(|_| {
            ConversionError("given epoch nanos are too large to fit into an Instant")
        })?;
        let subsecond_nanos = subsecond_nanos as u32; // safe cast because of the modulus
        Ok(Instant {
            seconds,
            subsecond_nanos,
        })
    }

    /// Returns the number of nanoseconds since the Unix epoch that this `Instant` represents.
    pub fn as_nanos(&self) -> i128 {
        let seconds = self.seconds as i128 * NANOS_PER_SECOND;
        if seconds < 0 {
            let adjusted_nanos = self.subsecond_nanos as i128 - NANOS_PER_SECOND;
            seconds + NANOS_PER_SECOND + adjusted_nanos
        } else {
            seconds + self.subsecond_nanos as i128
        }
    }

    /// Creates an `Instant` from a number of seconds and a fractional second since the Unix epoch.
    ///
    /// # Example
    /// ```
    /// # use aws_smithy_types::Instant;
    /// assert_eq!(
    ///     Instant::from_secs_and_nanos(1, 500_000_000u32),
    ///     Instant::from_fractional_secs(1, 0.5),
    /// );
    /// ```
    pub fn from_fractional_secs(epoch_seconds: i64, fraction: f64) -> Self {
        let subsecond_nanos = (fraction * 1_000_000_000_f64) as u32;
        Instant::from_secs_and_nanos(epoch_seconds, subsecond_nanos)
    }

    /// Creates an `Instant` from a number of seconds and sub-second nanos since the Unix epoch.
    ///
    /// # Example
    /// ```
    /// # use aws_smithy_types::Instant;
    /// assert_eq!(
    ///     Instant::from_fractional_secs(1, 0.5),
    ///     Instant::from_secs_and_nanos(1, 500_000_000u32),
    /// );
    /// ```
    pub fn from_secs_and_nanos(seconds: i64, subsecond_nanos: u32) -> Self {
        if subsecond_nanos >= 1_000_000_000 {
            panic!("{} is > 1_000_000_000", subsecond_nanos)
        }
        Instant {
            seconds,
            subsecond_nanos,
        }
    }

    /// Returns the `Instant` value as an `f64` representing the seconds since the Unix epoch.
    ///
    /// _Note: This conversion will lose precision due to the nature of floating point numbers._
    pub fn as_secs_f64(&self) -> f64 {
        self.seconds as f64 + self.subsecond_nanos as f64 / 1_000_000_000_f64
    }

    /// Creates an `Instant` from an `f64` representing the number of seconds since the Unix epoch.
    ///
    /// # Example
    /// ```
    /// # use aws_smithy_types::Instant;
    /// assert_eq!(
    ///     Instant::from_fractional_secs(1, 0.5),
    ///     Instant::from_secs_f64(1.5),
    /// );
    /// ```
    pub fn from_secs_f64(epoch_seconds: f64) -> Self {
        let seconds = epoch_seconds.floor() as i64;
        let rem = epoch_seconds - epoch_seconds.floor();
        Instant::from_fractional_secs(seconds, rem)
    }

    /// Parses an `Instant` from a string using the given `format`.
    pub fn from_str(s: &str, format: Format) -> Result<Self, InstantParseError> {
        match format {
            Format::DateTime => format::rfc3339::parse(s),
            Format::HttpDate => format::http_date::parse(s),
            Format::EpochSeconds => format::epoch_seconds::parse(s),
        }
    }

    /// Returns true if sub-second nanos is greater than zero.
    pub fn has_subsec_nanos(&self) -> bool {
        self.subsecond_nanos != 0
    }

    /// Returns the epoch seconds component of the `Instant`.
    ///
    /// _Note: this does not include the sub-second nanos._
    pub fn secs(&self) -> i64 {
        self.seconds
    }

    /// Returns the sub-second nanos component of the `Instant`.
    ///
    /// _Note: this does not include the number of seconds since the epoch._
    pub fn subsec_nanos(&self) -> u32 {
        self.subsecond_nanos
    }

    /// Converts the `Instant` to the number of milliseconds since the Unix epoch.
    ///
    /// This is fallible since `Instant` holds more precision than an `i64`, and will
    /// return a `ConversionError` for `Instant` values that can't be converted.
    pub fn to_millis(self) -> Result<i64, ConversionError> {
        let subsec_millis =
            Integer::div_floor(&i64::from(self.subsecond_nanos), &(NANOS_PER_MILLI as i64));
        if self.seconds < 0 {
            self.seconds
                .checked_add(1)
                .and_then(|seconds| seconds.checked_mul(MILLIS_PER_SECOND))
                .and_then(|millis| millis.checked_sub(1000 - subsec_millis))
        } else {
            self.seconds
                .checked_mul(MILLIS_PER_SECOND)
                .and_then(|millis| millis.checked_add(subsec_millis))
        }
        .ok_or(ConversionError(
            "Instant value too large to fit into i64 epoch millis",
        ))
    }

    /// Read 1 date of `format` from `s`, expecting either `delim` or EOF
    ///
    /// Enable parsing multiple dates from the same string
    pub fn read(s: &str, format: Format, delim: char) -> Result<(Self, &str), InstantParseError> {
        let (inst, next) = match format {
            Format::DateTime => format::rfc3339::read(s)?,
            Format::HttpDate => format::http_date::read(s)?,
            Format::EpochSeconds => {
                let split_point = s.find(delim).unwrap_or_else(|| s.len());
                let (s, rest) = s.split_at(split_point);
                (Self::from_str(s, format)?, rest)
            }
        };
        if next.is_empty() {
            Ok((inst, next))
        } else if next.starts_with(delim) {
            Ok((inst, &next[1..]))
        } else {
            Err(InstantParseError::Invalid(
                "didn't find expected delimiter".into(),
            ))
        }
    }

    /// Formats the `Instant` to a string using the given `format`.
    pub fn fmt(&self, format: Format) -> String {
        match format {
            Format::DateTime => format::rfc3339::format(&self),
            Format::EpochSeconds => format::epoch_seconds::format(&self),
            Format::HttpDate => format::http_date::format(&self),
        }
    }
}

impl From<Instant> for SystemTime {
    fn from(instant: Instant) -> Self {
        if instant.secs() < 0 {
            let mut secs = instant.secs().unsigned_abs();
            let mut nanos = instant.subsec_nanos();
            if instant.has_subsec_nanos() {
                // This is safe because we just went from a negative number to a positive and are subtracting
                secs -= 1;
                // This is safe because nanos are < 999,999,999
                nanos = NANOS_PER_SECOND_U32 - nanos;
            }
            // This will panic if secs == i64::MIN
            UNIX_EPOCH - Duration::new(secs, nanos)
        } else {
            UNIX_EPOCH + Duration::new(instant.secs().unsigned_abs(), instant.subsec_nanos())
        }
    }
}

impl From<SystemTime> for Instant {
    fn from(time: SystemTime) -> Self {
        if time < UNIX_EPOCH {
            let duration = UNIX_EPOCH.duration_since(time).expect("time < UNIX_EPOCH");
            let mut secs = -(duration.as_secs() as i128);
            let mut nanos = duration.subsec_nanos() as i128;
            if nanos != 0 {
                secs -= 1;
                nanos = NANOS_PER_SECOND - nanos;
            }
            Instant::from_nanos(secs * NANOS_PER_SECOND + nanos)
                .expect("SystemTime has same precision as Instant")
        } else {
            let duration = time.duration_since(UNIX_EPOCH).expect("UNIX_EPOCH <= time");
            Instant::from_secs_and_nanos(
                i64::try_from(duration.as_secs())
                    .expect("SystemTime has same precision as Instant"),
                duration.subsec_nanos(),
            )
        }
    }
}

/// Failure to convert an `Instant` to or from another type.
#[derive(Debug)]
#[non_exhaustive]
pub struct ConversionError(&'static str);

impl StdError for ConversionError {}

impl fmt::Display for ConversionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Formats for representing an `Instant` in the Smithy protocols.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    /// RFC-3339 Date Time.
    DateTime,
    /// Date format used by the HTTP `Date` header, specified in RFC-7231.
    HttpDate,
    /// Number of seconds since the Unix epoch formatted as a floating point.
    EpochSeconds,
}

#[cfg(test)]
mod test {
    use crate::instant::Format;
    use crate::Instant;
    use std::time::SystemTime;
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;

    #[test]
    fn test_fmt() {
        let instant = Instant::from_secs(1576540098);
        assert_eq!(instant.fmt(Format::DateTime), "2019-12-16T23:48:18Z");
        assert_eq!(instant.fmt(Format::EpochSeconds), "1576540098");
        assert_eq!(
            instant.fmt(Format::HttpDate),
            "Mon, 16 Dec 2019 23:48:18 GMT"
        );

        let instant = Instant::from_fractional_secs(1576540098, 0.52);
        assert_eq!(instant.fmt(Format::DateTime), "2019-12-16T23:48:18.52Z");
        assert_eq!(instant.fmt(Format::EpochSeconds), "1576540098.52");
        assert_eq!(
            instant.fmt(Format::HttpDate),
            "Mon, 16 Dec 2019 23:48:18.52 GMT"
        );
    }

    #[test]
    fn test_fmt_zero_seconds() {
        let instant = Instant::from_secs(1576540080);
        assert_eq!(instant.fmt(Format::DateTime), "2019-12-16T23:48:00Z");
        assert_eq!(instant.fmt(Format::EpochSeconds), "1576540080");
        assert_eq!(
            instant.fmt(Format::HttpDate),
            "Mon, 16 Dec 2019 23:48:00 GMT"
        );
    }

    #[test]
    fn test_read_single_http_date() {
        let s = "Mon, 16 Dec 2019 23:48:18 GMT";
        let (_, next) = Instant::read(s, Format::HttpDate, ',').expect("valid");
        assert_eq!(next, "");
    }

    #[test]
    fn test_read_single_float() {
        let s = "1576540098.52";
        let (_, next) = Instant::read(s, Format::EpochSeconds, ',').expect("valid");
        assert_eq!(next, "");
    }

    #[test]
    fn test_read_many_float() {
        let s = "1576540098.52,1576540098.53";
        let (_, next) = Instant::read(s, Format::EpochSeconds, ',').expect("valid");
        assert_eq!(next, "1576540098.53");
    }

    #[test]
    fn test_ready_many_http_date() {
        let s = "Mon, 16 Dec 2019 23:48:18 GMT,Tue, 17 Dec 2019 23:48:18 GMT";
        let (_, next) = Instant::read(s, Format::HttpDate, ',').expect("valid");
        assert_eq!(next, "Tue, 17 Dec 2019 23:48:18 GMT");
    }

    #[derive(Debug)]
    struct EpochMillisTestCase {
        rfc3339: &'static str,
        epoch_millis: i64,
        epoch_seconds: i64,
        epoch_subsec_nanos: u32,
    }

    // These test case values were generated from the following Kotlin JVM code:
    // ```kotlin
    // val instant = Instant.ofEpochMilli(<epoch milli value>);
    // println(DateTimeFormatter.ISO_DATE_TIME.format(instant.atOffset(ZoneOffset.UTC)))
    // println(instant.epochSecond)
    // println(instant.nano)
    // ```
    const EPOCH_MILLIS_TEST_CASES: &[EpochMillisTestCase] = &[
        EpochMillisTestCase {
            rfc3339: "2021-07-30T21:20:04.123Z",
            epoch_millis: 1627680004123,
            epoch_seconds: 1627680004,
            epoch_subsec_nanos: 123000000,
        },
        EpochMillisTestCase {
            rfc3339: "1918-06-04T02:39:55.877Z",
            epoch_millis: -1627680004123,
            epoch_seconds: -1627680005,
            epoch_subsec_nanos: 877000000,
        },
        EpochMillisTestCase {
            rfc3339: "+292278994-08-17T07:12:55.807Z",
            epoch_millis: i64::MAX,
            epoch_seconds: 9223372036854775,
            epoch_subsec_nanos: 807000000,
        },
        EpochMillisTestCase {
            rfc3339: "-292275055-05-16T16:47:04.192Z",
            epoch_millis: i64::MIN,
            epoch_seconds: -9223372036854776,
            epoch_subsec_nanos: 192000000,
        },
    ];

    #[test]
    fn to_millis() {
        for test_case in EPOCH_MILLIS_TEST_CASES {
            println!("Test case: {:?}", test_case);
            let instant =
                Instant::from_secs_and_nanos(test_case.epoch_seconds, test_case.epoch_subsec_nanos);
            assert_eq!(test_case.epoch_seconds, instant.secs());
            assert_eq!(test_case.epoch_subsec_nanos, instant.subsec_nanos());
            assert_eq!(test_case.epoch_millis, instant.to_millis().unwrap());
        }

        assert!(Instant::from_secs_and_nanos(i64::MAX, 0)
            .to_millis()
            .is_err());
    }

    #[test]
    fn from_millis() {
        for test_case in EPOCH_MILLIS_TEST_CASES {
            println!("Test case: {:?}", test_case);
            let instant = Instant::from_millis(test_case.epoch_millis);
            assert_eq!(test_case.epoch_seconds, instant.secs());
            assert_eq!(test_case.epoch_subsec_nanos, instant.subsec_nanos());
        }
    }

    #[test]
    fn to_from_millis_round_trip() {
        for millis in &[0, 1627680004123, -1627680004123, i64::MAX, i64::MIN] {
            assert_eq!(*millis, Instant::from_millis(*millis).to_millis().unwrap());
        }
    }

    #[test]
    fn as_nanos() {
        assert_eq!(
            -9_223_372_036_854_775_807_000_000_001_i128,
            Instant::from_secs_and_nanos(i64::MIN, 999_999_999).as_nanos()
        );
        assert_eq!(
            -10_876_543_211,
            Instant::from_secs_and_nanos(-11, 123_456_789).as_nanos()
        );
        assert_eq!(0, Instant::from_secs_and_nanos(0, 0).as_nanos());
        assert_eq!(
            11_123_456_789,
            Instant::from_secs_and_nanos(11, 123_456_789).as_nanos()
        );
        assert_eq!(
            9_223_372_036_854_775_807_999_999_999_i128,
            Instant::from_secs_and_nanos(i64::MAX, 999_999_999).as_nanos()
        );
    }

    #[test]
    fn from_nanos() {
        assert_eq!(
            Instant::from_secs_and_nanos(i64::MIN, 999_999_999),
            Instant::from_nanos(-9_223_372_036_854_775_807_000_000_001_i128).unwrap(),
        );
        assert_eq!(
            Instant::from_secs_and_nanos(-11, 123_456_789),
            Instant::from_nanos(-10_876_543_211).unwrap(),
        );
        assert_eq!(
            Instant::from_secs_and_nanos(0, 0),
            Instant::from_nanos(0).unwrap(),
        );
        assert_eq!(
            Instant::from_secs_and_nanos(11, 123_456_789),
            Instant::from_nanos(11_123_456_789).unwrap(),
        );
        assert_eq!(
            Instant::from_secs_and_nanos(i64::MAX, 999_999_999),
            Instant::from_nanos(9_223_372_036_854_775_807_999_999_999_i128).unwrap(),
        );
        assert!(Instant::from_nanos(-10_000_000_000_000_000_000_999_999_999_i128).is_err());
        assert!(Instant::from_nanos(10_000_000_000_000_000_000_999_999_999_i128).is_err());
    }

    #[test]
    fn system_time_conversions() {
        // Check agreement
        let instant = Instant::from_str("1000-01-02T01:23:10.123Z", Format::DateTime).unwrap();
        let date_time = OffsetDateTime::parse("1000-01-02T01:23:10.123Z", &Rfc3339).unwrap();
        assert_eq!(SystemTime::from(date_time), SystemTime::from(instant));

        let instant = Instant::from_str("2039-10-31T23:23:10.456Z", Format::DateTime).unwrap();
        let date_time = OffsetDateTime::parse("2039-10-31T23:23:10.456Z", &Rfc3339).unwrap();
        assert_eq!(SystemTime::from(date_time), SystemTime::from(instant));

        // Check boundaries and round-tripping
        let result = Instant::from(SystemTime::from(Instant::from_secs_and_nanos(
            i64::MAX,
            999_999_999,
        )));
        assert_eq!(i64::MAX, result.secs());
        assert_eq!(999_999_999, result.subsec_nanos());

        let result = Instant::from(SystemTime::from(Instant::from_secs_and_nanos(
            i64::MIN + 1,
            999_999_999,
        )));
        assert_eq!(i64::MIN + 1, result.secs());
        assert_eq!(999_999_999, result.subsec_nanos());
    }
}
