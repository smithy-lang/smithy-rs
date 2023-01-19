/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::*;
use serde::{Deserialize, Serialize};
use std::ffi::CString;

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
