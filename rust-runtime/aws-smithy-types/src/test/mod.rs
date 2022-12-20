/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::ffi::CString;
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, PartialEq)]
struct ForTest {
    blob: Blob,
}
#[test]
fn human_readable_blob() {
    let aws_in_base64 = r#"{"blob":"QVdT"}"#;
    let for_test = ForTest {
        blob: Blob {
            inner: vec![b'A', b'W', b'S'],
        },
    };
    assert_eq!(for_test, serde_json::from_str(aws_in_base64).unwrap());
    assert_eq!(serde_json::to_string(&for_test).unwrap(), aws_in_base64);
}

#[test]
fn not_human_readable_blob() {
    let for_test = ForTest {
        blob: Blob {
            inner: vec![b'A', b'W', b'S'],
        },
    };
    let mut buf = vec![];
    let res = ciborium::ser::into_writer(&for_test, &mut buf);
    assert!(res.is_ok());

    // checks whether the bytes are deserialiezd properly
    let n: HashMap<String, CString> =
        ciborium::de::from_reader(std::io::Cursor::new(buf.clone())).unwrap();
    assert!(n.get("blob").is_some());
    assert!(n.get("blob") == CString::new([65, 87, 83]).ok().as_ref());

    let de: ForTest = ciborium::de::from_reader(std::io::Cursor::new(buf)).unwrap();
    assert_eq!(for_test, de);
}

/// checks that it is serialized into a string formatted with RFC3339
#[test]
fn human_readable_datetime() {
    let datetime = DateTime::from_secs(1576540098);
    #[derive(Serialize, Deserialize, PartialEq)]
    struct Test {
        datetime: DateTime,
    }
    let datetime_json = r#"{"datetime":"2019-12-16T23:48:18Z"}"#;
    assert!(serde_json::to_string(&Test { datetime }).ok() == Some(datetime_json.to_string()));

    let test = serde_json::from_str::<Test>(&datetime_json).ok();
    assert!(test.is_some());
    assert!(test.unwrap().datetime == datetime);
}

/// checks that they are serialized into tuples
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
