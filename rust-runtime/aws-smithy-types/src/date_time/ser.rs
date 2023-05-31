/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::*;
use serde::ser::SerializeTuple;

impl serde::Serialize for DateTime {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if serializer.is_human_readable() {
            match self.fmt(Format::DateTime) {
                Ok(val) => serializer.serialize_str(&val),
                Err(e) => Err(serde::ser::Error::custom(e)),
            }
        } else {
            let mut tup_ser = serializer.serialize_tuple(2)?;
            tup_ser.serialize_element(&self.seconds)?;
            tup_ser.serialize_element(&self.subsecond_nanos)?;
            tup_ser.end()
        }
    }
}

/// check for human redable format
#[test]
fn human_readable_datetime() {
    use serde::{Deserialize, Serialize};

    let datetime = DateTime::from_secs(1576540098);
    #[derive(Serialize, Deserialize, PartialEq)]
    struct Test {
        datetime: DateTime,
    }
    let datetime_json = r#"{"datetime":"2019-12-16T23:48:18Z"}"#;
    assert!(serde_json::to_string(&Test { datetime }).ok() == Some(datetime_json.to_string()));
}

/// check for non-human redable format
#[test]
fn not_human_readable_datetime() {
    let cbor = ciborium::value::Value::Array(vec![
        ciborium::value::Value::Integer(1576540098i64.into()),
        ciborium::value::Value::Integer(0u32.into()),
    ]);
    let datetime = DateTime::from_secs(1576540098);

    let mut buf1 = vec![];
    let mut buf2 = vec![];
    let res1 = ciborium::ser::into_writer(&datetime, &mut buf1);
    let res2 = ciborium::ser::into_writer(&cbor, &mut buf2);
    assert!(res1.is_ok() && res2.is_ok());
    assert!(buf1 == buf2, "{:#?}", (buf1, buf2));
}
