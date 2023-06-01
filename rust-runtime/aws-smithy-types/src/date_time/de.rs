/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::*;
use serde::de::{Error, Visitor};
use serde::Deserialize;

struct DateTimeVisitor;

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
            Err(e) => Err(Error::custom(e)),
        }
    }
}

impl<'de> Deserialize<'de> for DateTime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(DateTimeVisitor)
    }
}

/// check for human redable format
#[test]
fn deser() {
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
