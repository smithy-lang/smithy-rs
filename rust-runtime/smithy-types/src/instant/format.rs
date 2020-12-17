/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

const NANOS_PER_SECOND: u32 = 1_000_000_000;

pub mod http_date {
    // This code is taken from https://github.com/pyfisch/httpdate and modified under an
    // Apache 2.0 License. Modifications:
    // - Removed use of unsafe
    // - Add serialization and deserialization of subsecond nanos
    use crate::Instant;
    use std::str::FromStr;
    use chrono::{Datelike, Weekday, Timelike, NaiveDateTime, NaiveDate, NaiveTime};
    use crate::instant::format::NANOS_PER_SECOND;

    /// Ok: "Mon, 16 Dec 2019 23:48:18 GMT"
    /// Ok: "Mon, 16 Dec 2019 23:48:18.123 GMT"
    /// Ok: "Mon, 16 Dec 2019 23:48:18.12 GMT"
    /// Not Ok: "Mon, 16 Dec 2019 23:48:18.1234 GMT"
    pub fn format(instant: &Instant) -> String {
        let structured = instant.to_chrono();
        let weekday = match structured.weekday() {
            Weekday::Mon => "Mon",
            Weekday::Tue => "Tue",
            Weekday::Wed => "Wed",
            Weekday::Thu => "Thu",
            Weekday::Fri => "Fri",
            Weekday::Sat => "Sat",
            Weekday::Sun => "Sun"
        };
        let month = match structured.month() {
            1 => "Jan",
            2 => "Feb",
            3 => "Mar",
            4 => "Apr",
            5 => "May",
            6 => "Jun",
            7 => "Jul",
            8 => "Aug",
            9 => "Sep",
            10 => "Oct",
            11 => "Nov",
            12 => "Dec",
            _ => unreachable!(),
        };
        let mut out = String::with_capacity(32);
        fn push_digit(out: &mut String, digit: u8) {
            out.push((b'0' + digit as u8) as char);
        }

        out.push_str(weekday);
        out.push_str(", ");
        let day = structured.date().day() as u8;
        push_digit(&mut out, day / 10);
        push_digit(&mut out, day % 10);

        out.push(' ');
        out.push_str(month);

        out.push(' ');

        let year = structured.year();
        let year = if year < 0 {
            panic!("negative years not supported")
        } else {
            year as u32
        };
        push_digit(&mut out, (year / 1000) as u8);
        push_digit(&mut out, (year / 100 % 10) as u8);
        push_digit(&mut out, (year / 10 % 10) as u8);
        push_digit(&mut out, (year % 10) as u8);

        out.push(' ');

        let hour = structured.time().hour() as u8;
        push_digit(&mut out, hour / 10);
        push_digit(&mut out, hour % 10);

        out.push(':');

        let minute = structured.minute() as u8;
        push_digit(&mut out, minute / 10);
        push_digit(&mut out, minute % 10);

        out.push(':');

        let second = structured.second() as u8;
        push_digit(&mut out, second / 10);
        push_digit(&mut out, second % 10);

        let nanos = structured.timestamp_subsec_nanos();
        if nanos != 0 {
            out.push('.');
            push_digit(&mut out, (nanos / (NANOS_PER_SECOND / 10)) as u8);
            push_digit(
                &mut out,
                (nanos / (NANOS_PER_SECOND / 100) % 10) as u8,
            );
            push_digit(
                &mut out,
                (nanos / (NANOS_PER_SECOND / 1000) % 10) as u8,
            );
        }

        out.push_str(" GMT");

        out
    }

    #[derive(Debug, Eq, PartialEq)]
    pub enum DateParseError {
        Invalid(&'static str),
        IntParseError,
    }

    pub fn parse(s: &str) -> Result<Instant, DateParseError> {
        if !s.is_ascii() {
            return Err(DateParseError::Invalid("not ascii"));
        }
        let x = s.trim().as_bytes();
        parse_imf_fixdate(x)
    }

    fn parse_imf_fixdate(s: &[u8]) -> Result<Instant, DateParseError> {
        // Example: `Sun, 06 Nov 1994 08:49:37 GMT`
        if s.len() < 29
            || s.len() > 33
            || !s.ends_with(b" GMT")
            || s[16] != b' '
            || s[19] != b':'
            || s[22] != b':'
        {
            return Err(DateParseError::Invalid("incorrectly shaped string"));
        }
        let nanos: u32 = match &s[25] {
            b'.' => {
                // The date must end with " GMT", so read from the character after the `.`
                // to 4 from the end
                let fraction_slice = &s[26..s.len() - 4];
                if fraction_slice.len() > 3 {
                    // Only thousandths are supported
                    return Err(DateParseError::Invalid("too much precision"));
                }
                let fraction: u32 = parse_slice(fraction_slice)?;
                // We need to convert the fractional second to nanoseconds, so we need to scale
                // according the the number of decimals provided
                let multiplier = [10, 100, 100];
                fraction * (NANOS_PER_SECOND / multiplier[fraction_slice.len() - 1])
            }
            b' ' => 0,
            _ => return Err(DateParseError::Invalid("incorrectly shaped string")),
        };

        let time = NaiveTime::from_hms_nano(
            parse_slice(&s[17..19])?,
            parse_slice(&s[20..22])?,
            parse_slice(&s[23..25])?,
            nanos,
        );
        let month = match &s[7..12] {
            b" Jan " => 1,
            b" Feb " => 2,
            b" Mar " => 3,
            b" Apr " => 4,
            b" May " => 5,
            b" Jun " => 6,
            b" Jul " => 7,
            b" Aug " => 8,
            b" Sep " => 9,
            b" Oct " => 10,
            b" Nov " => 11,
            b" Dec " => 12,
            _ => return Err(DateParseError::Invalid("invalid month")),
        };
        let date = NaiveDate::from_ymd(parse_slice(&s[12..16])?,
                                       month, parse_slice(&s[5..7])?,
        );
        let datetime = NaiveDateTime::new(date, time);

        Ok(
            Instant::from_secs_and_nanos(datetime.timestamp(), datetime.timestamp_subsec_nanos())
        )
    }

    fn parse_slice<T>(ascii_slice: &[u8]) -> Result<T, DateParseError>
        where
            T: FromStr,
    {
        let as_str =
            std::str::from_utf8(ascii_slice).expect("should only be called on ascii strings");
        as_str
            .parse::<T>()
            .map_err(|_| DateParseError::IntParseError)
    }
}

#[cfg(test)]
mod test {
    use crate::instant::format::http_date;
    use crate::instant::format::http_date::DateParseError;
    use crate::Instant;

    #[test]
    fn http_date_format() {
        let basic_http_date = "Mon, 16 Dec 2019 23:48:18 GMT";
        let ts = 1576540098;
        let instant = Instant::from_epoch_seconds(ts);
        assert_eq!(http_date::format(&instant), basic_http_date);
        assert_eq!(http_date::parse(basic_http_date), Ok(instant));
    }

    #[test]
    fn http_date_format_fractional_zeroed() {
        let basic_http_date = "Mon, 16 Dec 2019 23:48:18 GMT";
        let fractional = "Mon, 16 Dec 2019 23:48:18.000 GMT";
        let ts = 1576540098;
        let instant = Instant::from_epoch_seconds(ts);
        assert_eq!(http_date::format(&instant), basic_http_date);
        assert_eq!(http_date::parse(fractional), Ok(instant));
    }

    #[test]
    fn http_date_format_fractional_nonzero() {
        let fractional = "Mon, 16 Dec 2019 23:48:18.12 GMT";
        let fractional_normalized = "Mon, 16 Dec 2019 23:48:18.120 GMT";
        let ts = 1576540098;
        let instant = Instant::from_fractional_seconds(ts, 0.12);
        assert_eq!(http_date::parse(fractional), Ok(instant));
        assert_eq!(http_date::format(&instant), fractional_normalized);
    }

    #[test]
    fn too_much_fraction() {
        let fractional = "Mon, 16 Dec 2019 23:48:18.1212 GMT";
        assert_eq!(
            http_date::parse(fractional),
            Err(DateParseError::Invalid("incorrectly shaped string"))
        );
    }

    #[test]
    fn no_fraction() {
        let fractional = "Mon, 16 Dec 2019 23:48:18. GMT";
        assert_eq!(
            http_date::parse(fractional),
            Err(DateParseError::IntParseError)
        );
    }

    #[track_caller]
    fn check_roundtrip(epoch_secs: i64) {
        let instant = Instant::from_epoch_seconds(epoch_secs);
        let http_date = http_date::format(&instant);
        assert_eq!(http_date::parse(&http_date), Ok(instant), "{}", http_date);
    }

    #[test]
    fn http_date_roundtrip() {
        for epoch_secs in 0..1000 {
            check_roundtrip(epoch_secs);
        }

        check_roundtrip(1576540098);
        check_roundtrip(9999999999);
    }
}
