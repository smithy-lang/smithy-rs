/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_auth::Credentials;
use aws_hyper::test_connection::TestConnection;
use aws_hyper::{Client};
use aws_sdk_qldbsession as qldbsession;
use http::Uri;
use qldbsession::model::StartSessionRequest;
use qldbsession::operation::{SendCommand};
use qldbsession::{Config, Region};
use smithy_http::body::SdkBody;

// TODO: having the full HTTP requests right in the code is a bit gross, consider something
// like https://github.com/davidbarsky/sigv4/blob/master/aws-sigv4/src/lib.rs#L283-L315 to store
// the requests/responses externally

#[tokio::test]
async fn signv4_use_correct_service_name() {
    let creds = Credentials::from_keys(
        "ANOTREAL",
        "notrealrnrELgWzOk3IfjzDKtFBhDby",
        Some("notarealsessiontoken".to_string()),
    );
    let conn = TestConnection::new(vec![(
        http::Request::builder()
            .header("content-type", "application/x-amz-json-1.1")
            .header("x-amz-target", "TrentService.GenerateRandom")
            .header("content-length", "20")
            .header("host", "qldbsession.us-east-1.amazonaws.com")
            .header("authorization", "AWS4-HMAC-SHA256 Credential=ANOTREAL/20210305/us-east-1/qldb/aws4_request, SignedHeaders=content-length;content-type;host;x-amz-target, Signature=750c6333c96dcbe4c4c11a9af8483ff68ac40e0e8ba8244772d981aab3cda703")
            // qldbsession uses the service name 'qldb' in signature _________________________^^^^
            .header("x-amz-date", "20210305T134922Z")
            .header("x-amz-security-token", "notarealsessiontoken")
            .header("user-agent", "aws-sdk-rust/0.123.test os/windows/XPSP3 lang/rust/1.50.0")
            .header("x-amz-user-agent", "aws-sdk-rust/0.123.test api/test-service/0.123 os/windows/XPSP3 lang/rust/1.50.0")
            .uri(Uri::from_static("https://qldbsession.us-east-1.amazonaws.com/"))
            .body(SdkBody::from(r#"{"NumberOfBytes":64}"#)).unwrap(),
        http::Response::builder()
            .status(http::StatusCode::from_u16(200).unwrap())
            .body(r#"{"Plaintext":"6CG0fbzzhg5G2VcFCPmJMJ8Njv3voYCgrGlp3+BZe7eDweCXgiyDH9BnkKvLmS7gQhnYDUlyES3fZVGwv5+CxA=="}"#).unwrap())
    ]);
    let client = Client::new(conn.clone());
    let conf = Config::builder()
        .region(Region::new("us-east-1"))
        .credentials_provider(creds)
        .build();

    let op = SendCommand::builder()
        .start_session(
            StartSessionRequest::builder()
                .ledger_name("not-real-ledger")
                .build()
        )
        .build()
        .unwrap()
        .make_operation(&conf)
        .expect("valid operation");

    let _ = client.call(op).await.expect("request should succeed");

    assert_eq!(conn.requests().len(), 1);
    for validate_request in conn.requests().iter() {
        validate_request.assert_matches(vec![]);
    }
}
