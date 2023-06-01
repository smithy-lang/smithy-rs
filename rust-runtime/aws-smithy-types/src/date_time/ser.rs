/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::*;

impl serde::Serialize for DateTime {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self.fmt(Format::DateTime) {
            Ok(val) => serializer.serialize_str(&val),
            Err(e) => Err(serde::ser::Error::custom(e)),
        }
    }
}

/// check for human redable format
#[test]
fn serde() {
    use serde::{Deserialize, Serialize};

    let datetime = DateTime::from_secs(1576540098);
    #[derive(Serialize, Deserialize, PartialEq)]
    struct Test {
        datetime: DateTime,
    }
    let datetime_json = r#"{"datetime":"2019-12-16T23:48:18Z"}"#;
    assert!(serde_json::to_string(&Test { datetime }).ok() == Some(datetime_json.to_string()));
}
