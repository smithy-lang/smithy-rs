/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::convert::Infallible;
use std::error::Error;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use bytes::Bytes;
use http::header::{AUTHORIZATION, USER_AGENT};
use http::{self, Uri};

use aws_endpoint::partition::endpoint::{Protocol, SignatureVersion};
use aws_endpoint::set_endpoint_resolver;
use aws_http::retry::AwsErrorRetryPolicy;
use aws_http::user_agent::AwsUserAgent;
use aws_inlineable::middleware::DefaultMiddleware;
use aws_sig_auth::signer::OperationSigningConfig;

use aws_smithy_client::test_connection::TestConnection;
use aws_smithy_http::body::SdkBody;
use aws_smithy_http::operation;
use aws_smithy_http::operation::Operation;
use aws_smithy_http::response::ParseHttpResponse;

use aws_smithy_types::retry::{ErrorKind, ProvideErrorKind};
use aws_types::credentials::SharedCredentialsProvider;
use aws_types::region::Region;
use aws_types::Credentials;
use aws_types::SigningService;

type Client<C> = aws_smithy_client::Client<C, DefaultMiddleware>;

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

impl ProvideErrorKind for OperationError {
    fn retryable_error_kind(&self) -> Option<ErrorKind> {
        Some(ErrorKind::ThrottlingError)
    }

    fn code(&self) -> Option<&str> {
        None
    }
}

impl ParseHttpResponse for TestOperationParser {
    type Output = Result<String, OperationError>;

    fn parse_unloaded(&self, response: &mut operation::Response) -> Option<Self::Output> {
        if response.http().status().is_success() {
            Some(Ok("Hello!".to_string()))
        } else {
            Some(Err(OperationError))
        }
    }

    fn parse_loaded(&self, _response: &http::Response<Bytes>) -> Self::Output {
        Ok("Hello!".to_string())
    }
}

fn test_operation() -> Operation<TestOperationParser, AwsErrorRetryPolicy> {
    let req = operation::Request::new(
        http::Request::builder()
            .uri("https://test-service.test-region.amazonaws.com/")
            .body(SdkBody::from("request body"))
            .unwrap(),
    )
    .augment(|req, mut conf| {
        set_endpoint_resolver(
            &mut conf,
            Arc::new(aws_endpoint::partition::endpoint::Metadata {
                uri_template: "test-service.{region}.amazonaws.com",
                protocol: Protocol::Https,
                credential_scope: Default::default(),
                signature_versions: SignatureVersion::V4,
            }),
        );
        aws_http::auth::set_provider(
            &mut conf,
            SharedCredentialsProvider::new(Credentials::new(
                "access_key",
                "secret_key",
                None,
                None,
                "test",
            )),
        );
        conf.insert(Region::new("test-region"));
        conf.insert(OperationSigningConfig::default_config());
        conf.insert(SigningService::from_static("test-service-signing"));
        conf.insert(UNIX_EPOCH + Duration::from_secs(1613414417));
        conf.insert(AwsUserAgent::for_tests());
        Result::<_, Infallible>::Ok(req)
    })
    .unwrap();
    Operation::new(req, TestOperationParser).with_retry_policy(AwsErrorRetryPolicy::new())
}

#[cfg(any(feature = "native-tls", feature = "rustls"))]
#[test]
fn test_default_client() {
    let client = Client::dyn_https();
    let _ = client.call(test_operation());
}

#[tokio::test]
async fn e2e_test() {
    let expected_req = http::Request::builder()
        .header(USER_AGENT, "aws-sdk-rust/0.123.test os/windows/XPSP3 lang/rust/1.50.0")
        .header("x-amz-user-agent", "aws-sdk-rust/0.123.test api/test-service/0.123 os/windows/XPSP3 lang/rust/1.50.0")
        .header(AUTHORIZATION, "AWS4-HMAC-SHA256 Credential=access_key/20210215/test-region/test-service-signing/aws4_request, SignedHeaders=host;x-amz-date;x-amz-user-agent, Signature=da249491d7fe3da22c2e09cbf910f37aa5b079a3cedceff8403d0b18a7bfab75")
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

    conn.assert_requests_match(&[]);
}
