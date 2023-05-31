/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::*;
use serde::de::Visitor;
use serde::Deserialize;

struct DateTimeVisitor;

enum VisitorState {
    Second,
    SubsecondNanos,
}

impl VisitorState {
    const UNEXPECTED_VISITOR_STATE: &'static str = "Unexpected state. This happens when visitor tries to parse something after finished parsing the `subsec_nanos`.";
}

struct NonHumanReadableDateTimeVisitor {
    state: VisitorState,
    seconds: i64,
    subsecond_nanos: u32,
}

fn fail<T, M, E>(err_message: M) -> Result<T, E>
where
    M: std::fmt::Display,
    E: serde::de::Error,
{
    Err(E::custom(err_message))
}

impl<'de> Visitor<'de> for DateTimeVisitor {
    type Value = DateTime;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("expected RFC-3339 Date Time")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match DateTime::from_str(v, Format::DateTime) {
            Ok(e) => Ok(e),
            Err(e) => fail(e),
        }
    }
}

impl<'de> Visitor<'de> for NonHumanReadableDateTimeVisitor {
    type Value = Self;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("expected (i64, u32)")
    }

    fn visit_i64<E>(mut self, v: i64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match self.state {
            VisitorState::Unexpected => fail(VisitorState::UNEXPECTED_VISITOR_STATE),
            VisitorState::Second => {
                self.seconds = v;
                self.state = VisitorState::SubsecondNanos;
                Ok(self)
            }
            _ => fail("`seconds` value must be i64"),
        }
    }

    fn visit_u32<E>(mut self, v: u32) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match self.state {
            VisitorState::Unexpected => fail(VisitorState::UNEXPECTED_VISITOR_STATE),
            VisitorState::SubsecondNanos => {
                self.subsecond_nanos = v;
                Ok(self)
            }
            _ => fail("`subsecond_nanos` value must be u32"),
        }
    }
}

impl<'de> Deserialize<'de> for DateTime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            deserializer.deserialize_str(DateTimeVisitor)
        } else {
            let visitor = NonHumanReadableDateTimeVisitor {
                state: VisitorState::Second,
                seconds: 0,
                subsecond_nanos: 0,
            };
            let visitor = deserializer.deserialize_tuple(2, visitor)?;
            Ok(DateTime {
                seconds: visitor.seconds,
                subsecond_nanos: visitor.subsecond_nanos,
            })
        }
    }
}

/// checks the value can be serialized/de-serialized in human readable datetime format
#[test]
fn human_readable_datetime() {
    use serde::{Deserialize, Serialize};

    let datetime = DateTime::from_secs(1576540098);
    #[derive(Serialize, Deserialize, PartialEq)]
    struct Test {
        datetime: DateTime,
    }
    let datetime_json = r#"{"datetime":"2019-12-16T23:48:18Z"}"#;
    let test = serde_json::from_str::<Test>(&datetime_json).ok();
    assert!(test == Some(Test { datetime }));
}

/// checks the value can be serialized/deserialized into tuples
#[test]
fn not_human_readable_datetime() {
    let cbor = ciborium::value::Value::Array(vec![
        ciborium::value::Value::Integer(1576540098i64.into()),
        ciborium::value::Value::Integer(0u32.into()),
    ]);
    let datetime = DateTime::from_secs(1576540098);

    let mut buf1 = vec![];
    let mut buf2 = vec![];
    let _ = ciborium::ser::into_writer(&datetime, &mut buf1);
    let res = ciborium::de::from_reader(std::io::Cursor::new(buf1));
    assert_eq!(res == Ok(datetime));
}
