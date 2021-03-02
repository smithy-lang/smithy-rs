/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_auth::Credentials;
use aws_endpoint::{set_endpoint_resolver, DefaultAwsEndpointResolver};
use aws_http::user_agent::AwsUserAgent;
use aws_hyper::test_connection::{TestConnection, ValidateRequest};
use aws_hyper::Client;
use aws_sig_auth::signer::OperationSigningConfig;
use aws_types::region::Region;
use bytes::Bytes;
use http::header::{AUTHORIZATION, HOST, USER_AGENT};
use http::{Response, Uri};
use smithy_http::body::SdkBody;
use smithy_http::operation;
use smithy_http::operation::Operation;
use smithy_http::response::ParseHttpResponse;
use std::convert::Infallible;
use std::error::Error;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

#[derive(Clone)]
struct TestOperationParser;

#[derive(Debug)]
struct OperationError;
impl Display for OperationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Error for OperationError {}

impl<B> ParseHttpResponse<B> for TestOperationParser
where
    B: http_body::Body,
{
    type Output = Result<String, OperationError>;

    fn parse_unloaded(&self, _response: &mut Response<B>) -> Option<Self::Output> {
        Some(Ok("Hello!".to_string()))
    }

    fn parse_loaded(&self, _response: &Response<Bytes>) -> Self::Output {
        Ok("Hello!".to_string())
    }
}

fn test_operation() -> Operation<TestOperationParser, ()> {
    let req = operation::Request::new(http::Request::new(SdkBody::from("request body")))
        .augment(|req, mut conf| {
            set_endpoint_resolver(
                &mut conf,
                Arc::new(DefaultAwsEndpointResolver::for_service("test-service")),
            );
            aws_auth::set_provider(
                &mut conf,
                Arc::new(Credentials::from_keys("access_key", "secret_key", None)),
            );
            conf.insert(Region::new("test-region"));
            conf.insert(OperationSigningConfig::default_config());
            conf.insert(UNIX_EPOCH + Duration::from_secs(1613414417));
            conf.insert(AwsUserAgent::for_tests());
            Result::<_, Infallible>::Ok(req)
        })
        .unwrap();
    Operation::new(req, TestOperationParser)
}

#[tokio::test]
async fn e2e_test() {
    let expected_req = http::Request::builder()
        .header(USER_AGENT, "aws-sdk-rust/0.123.test os/windows/XPSP3 lang/rust/1.50.0")
        .header("x-amz-user-agent", "aws-sdk-rust/0.123.test api/test-service/0.123 os/windows/XPSP3 lang/rust/1.50.0")
        .header(HOST, "test-service.test-region.amazonaws.com")
        .header(AUTHORIZATION, "AWS4-HMAC-SHA256 Credential=access_key/20210215/test-region/test-service/aws4_request, SignedHeaders=host, Signature=b4bccc6f03b22e88b9e52a60314d4629c5d159a7cc2de25b1d687b3e5e480d2c")
        .header("x-amz-date", "20210215T184017Z")
        .uri(Uri::from_static("https://test-service.test-region.amazonaws.com/"))
        .body(SdkBody::from("request body")).unwrap();
    let events = vec![(
        expected_req,
        http::Response::builder()
            .status(200)
            .body("response body")
            .unwrap(),
    )];
    let conn = TestConnection::new(events);
    let client = Client::new(conn.clone());
    let resp = client.call(test_operation()).await;
    let resp = resp.expect("successful operation");
    assert_eq!(resp, "Hello!");

    assert_eq!(conn.requests().len(), 1);
    let ValidateRequest { expected, actual } = &conn.requests()[0];
    assert_eq!(actual.headers(), expected.headers());
    assert_eq!(actual.body().bytes(), expected.body().bytes());
    assert_eq!(actual.uri(), expected.uri());
}
