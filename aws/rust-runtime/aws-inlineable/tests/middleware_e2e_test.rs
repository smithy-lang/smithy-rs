/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::convert::Infallible;
use std::error::Error;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::time::{Duration, UNIX_EPOCH};

use aws_credential_types::cache::CredentialsCache;
use aws_credential_types::provider::SharedCredentialsProvider;
use aws_credential_types::Credentials;
use aws_smithy_client::erase::DynConnector;
use aws_smithy_client::test_connection::TestConnection;
use aws_smithy_http::body::SdkBody;
use aws_smithy_http::operation;
use aws_smithy_http::operation::Operation;
use aws_smithy_http::response::ParseHttpResponse;
use aws_smithy_types::endpoint::Endpoint;
use aws_smithy_types::retry::{ErrorKind, ProvideErrorKind};
use bytes::Bytes;
use http::header::{AUTHORIZATION, USER_AGENT};
use http::{self, Uri};

use aws_http::retry::AwsResponseRetryClassifier;
use aws_http::user_agent::AwsUserAgent;
use aws_inlineable::middleware::DefaultMiddleware;
use aws_sig_auth::signer::OperationSigningConfig;
use aws_smithy_async::time::SharedTimeSource;
use aws_types::region::SigningRegion;
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

fn test_operation() -> Operation<TestOperationParser, AwsResponseRetryClassifier> {
    let req = operation::Request::new(
        http::Request::builder()
            .uri("/")
            .body(SdkBody::from("request body"))
            .unwrap(),
    )
    .augment(|req, conf| {
        conf.insert(aws_smithy_http::endpoint::Result::Ok(
            Endpoint::builder()
                .url("https://test-service.test-region.amazonaws.com")
                .build(),
        ));
        aws_http::auth::set_credentials_cache(
            conf,
            CredentialsCache::lazy()
                .create_cache(SharedCredentialsProvider::new(Credentials::for_tests())),
        );
        conf.insert(SigningRegion::from_static("test-region"));
        conf.insert(OperationSigningConfig::default_config());
        conf.insert(SigningService::from_static("test-service-signing"));
        conf.insert(SharedTimeSource::new(
            UNIX_EPOCH + Duration::from_secs(1613414417),
        ));
        conf.insert(AwsUserAgent::for_tests());
        Result::<_, Infallible>::Ok(req)
    })
    .unwrap();
    Operation::new(req, TestOperationParser)
        .with_retry_classifier(AwsResponseRetryClassifier::new())
        .with_metadata(operation::Metadata::new("test-op", "test-service"))
}

#[cfg(feature = "rustls")]
#[test]
fn test_default_client() {
    let client = Client::builder()
        .dyn_https_connector(Default::default())
        .middleware_fn(|r| r)
        .build();
    let _ = client.call(test_operation());
}

#[tokio::test]
async fn e2e_test() {
    let expected_req = http::Request::builder()
        .header(USER_AGENT, "aws-sdk-rust/0.123.test os/windows/XPSP3 lang/rust/1.50.0")
        .header("x-amz-user-agent", "aws-sdk-rust/0.123.test api/test-service/0.123 os/windows/XPSP3 lang/rust/1.50.0")
        .header(AUTHORIZATION, "AWS4-HMAC-SHA256 Credential=ANOTREAL/20210215/test-region/test-service-signing/aws4_request, SignedHeaders=host;x-amz-date;x-amz-security-token;x-amz-user-agent, Signature=6d477055738c4e634c2451b9fc378b6ff2f967d37657c3dd50a1b6a735576960")
        .header("x-amz-date", "20210215T184017Z")
        .header("x-amz-security-token", "notarealsessiontoken")
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

#[tokio::test]
async fn test_operation_metadata_is_available_to_middlewares() {
    let conn = TestConnection::new(vec![(
        http::Request::builder()
            .header(USER_AGENT, "aws-sdk-rust/0.123.test os/windows/XPSP3 lang/rust/1.50.0")
            .header("x-amz-user-agent", "aws-sdk-rust/0.123.test api/test-service/0.123 os/windows/XPSP3 lang/rust/1.50.0")
            .header(AUTHORIZATION, "AWS4-HMAC-SHA256 Credential=ANOTREAL/20210215/test-region/test-service-signing/aws4_request, SignedHeaders=host;x-amz-date;x-amz-security-token;x-amz-user-agent, Signature=6d477055738c4e634c2451b9fc378b6ff2f967d37657c3dd50a1b6a735576960")
            .header("x-amz-date", "20210215T184017Z")
            .header("x-amz-security-token", "notarealsessiontoken")
            .uri(Uri::from_static("https://test-service.test-region.amazonaws.com/"))
            .body(SdkBody::from("request body")).unwrap(),
        http::Response::builder()
            .status(200)
            .body("response body")
            .unwrap(),
    )]);
    let client = aws_smithy_client::Client::builder()
        .middleware_fn(|req| {
            let metadata = req
                .properties()
                .get::<operation::Metadata>()
                .cloned()
                .unwrap();

            assert_eq!("test-op", metadata.name());
            assert_eq!("test-service", metadata.service());

            req
        })
        .connector(DynConnector::new(conn))
        .build();

    let resp = client.call(test_operation()).await;
    let resp = resp.expect("successful operation");
    assert_eq!(resp, "Hello!");
}
