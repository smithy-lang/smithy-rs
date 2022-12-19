/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_s3::error::GetObjectErrorKind;
use aws_sdk_s3::operation::GetObject;
use aws_sdk_s3::types::RequestId;
use aws_smithy_http::response::ParseHttpResponse;
use bytes::Bytes;

#[test]
fn get_request_id_from_modeled_error() {
    let resp = http::Response::builder()
        .header("x-amz-request-id", "correct-request-id")
        .status(404)
        .body(
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <Error>
              <Code>NoSuchKey</Code>
              <Message>The resource you requested does not exist</Message>
              <Resource>/mybucket/myfoto.jpg</Resource>
              <RequestId>incorrect-request-id</RequestId>
            </Error>"#,
        )
        .unwrap();
    let err = GetObject::new()
        .parse_loaded(&resp.map(Bytes::from))
        .expect_err("status was 404, this is an error");
    assert!(matches!(err.kind, GetObjectErrorKind::NoSuchKey(_)));
    assert_eq!(Some("correct-request-id"), err.request_id());
    assert_eq!(Some("correct-request-id"), err.meta().request_id());
}

#[test]
fn get_request_id_from_unmodeled_error() {
    let resp = http::Response::builder()
        .header("x-amz-request-id", "correct-request-id")
        .status(500)
        .body(
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <Error>
              <Code>SomeUnmodeledError</Code>
              <Message>Something bad happened</Message>
              <Resource>/mybucket/myfoto.jpg</Resource>
              <RequestId>incorrect-request-id</RequestId>
            </Error>"#,
        )
        .unwrap();
    let err = GetObject::new()
        .parse_loaded(&resp.map(Bytes::from))
        .expect_err("status 500");
    assert!(matches!(err.kind, GetObjectErrorKind::Unhandled(_)));
    assert_eq!(Some("correct-request-id"), err.request_id());
    assert_eq!(Some("correct-request-id"), err.meta().request_id());
}
