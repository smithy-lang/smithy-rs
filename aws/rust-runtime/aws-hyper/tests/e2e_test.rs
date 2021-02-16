/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_auth::Credentials;
use aws_endpoint::{set_endpoint_resolver, DefaultAwsEndpointResolver};
use aws_hyper::test_connection::{TestConnection, ValidateRequest};
use aws_hyper::Client;
use aws_sig_auth::signer::OperationSigningConfig;
use aws_types::region::Region;
use bytes::Bytes;
use http::header::AUTHORIZATION;
use http::{Response, Uri};
use smithy_http::body::SdkBody;
use smithy_http::operation;
use smithy_http::operation::Operation;
use smithy_http::response::ParseHttpResponse;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

#[derive(Clone)]
struct TestOperationParser;

impl<B> ParseHttpResponse<B> for TestOperationParser
where
    B: http_body::Body,
{
    type Output = Result<String, String>;

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
            Result::<_, Infallible>::Ok(req)
        })
        .unwrap();
    Operation::new(req, TestOperationParser)
}

#[tokio::test]
async fn e2e_test() {
    let expected_req = http::Request::builder()
        .header(AUTHORIZATION, "AWS4-HMAC-SHA256 Credential=access_key/20210215/test-region/test-service/aws4_request, SignedHeaders=, Signature=e8a49c07c540558c4b53a5dcc61cbfb27003381fd8437fca0b3dddcdc703ec44")
        .header("x-amz-date", "20210215T184017Z")
        .uri(Uri::from_static("https://test-region.test-service.amazonaws.com/"))
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
