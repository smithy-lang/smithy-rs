/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_lambda::error::ListFunctionsErrorKind;
use aws_sdk_lambda::operation::ListFunctions;
use aws_sdk_lambda::types::RequestId;
use aws_smithy_http::response::ParseHttpResponse;
use bytes::Bytes;

#[test]
fn get_request_id_from_unmodeled_error() {
    let resp = http::Response::builder()
        .header("x-amzn-RequestId", "correct-request-id")
        .header("X-Amzn-Errortype", "ListFunctions")
        .status(500)
        .body("{}")
        .unwrap();
    let err = ListFunctions::new()
        .parse_loaded(&resp.map(Bytes::from))
        .expect_err("status was 500, this is an error");
    dbg!(&err);
    assert!(matches!(err.kind, ListFunctionsErrorKind::Unhandled(_)));
    assert_eq!(Some("correct-request-id"), err.request_id());
    assert_eq!(Some("correct-request-id"), err.meta().request_id());
}
