/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![cfg(all(feature = "client", feature = "wire-mock",))]

use ::aws_smithy_runtime::client::retries::classifiers::{
    HttpStatusCodeClassifier, TransientErrorClassifier,
};
use aws_smithy_async::rt::sleep::TokioSleep;
use aws_smithy_runtime::client::http::hyper_014::HyperClientBuilder;
use aws_smithy_runtime::client::http::test_util::wire::{
    RecordedEvent, ReplayedEvent, WireMockServer,
};
use aws_smithy_runtime::client::orchestrator::operation::Operation;
use aws_smithy_runtime::test_util::capture_test_logs::capture_test_logs;
use aws_smithy_runtime::{ev, match_events};
use aws_smithy_runtime_api::client::http::SharedHttpClient;
use aws_smithy_runtime_api::client::interceptors::context::InterceptorContext;
use aws_smithy_runtime_api::client::orchestrator::OrchestratorError;
use aws_smithy_runtime_api::client::retries::classifiers::{ClassifyRetry, RetryAction};
use aws_smithy_runtime_api::shared::IntoShared;
use aws_smithy_types::body::SdkBody;
use aws_smithy_types::retry::{ErrorKind, ProvideErrorKind, ReconnectMode, RetryConfig};
use aws_smithy_types::timeout::TimeoutConfig;
use std::fmt;
use std::time::Duration;

const END_OF_TEST: &str = "end_of_test";

#[derive(Debug)]
struct OperationError(ErrorKind);

impl fmt::Display for OperationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl ProvideErrorKind for OperationError {
    fn retryable_error_kind(&self) -> Option<ErrorKind> {
        Some(self.0)
    }

    fn code(&self) -> Option<&str> {
        None
    }
}

impl std::error::Error for OperationError {}

#[derive(Debug)]
struct TestRetryClassifier;

impl ClassifyRetry for TestRetryClassifier {
    fn classify_retry(&self, ctx: &InterceptorContext) -> RetryAction {
        tracing::info!("classifying retry for {ctx:?}");
        let output_or_error = ctx.output_or_error();
        let error = match output_or_error {
            Some(Ok(_)) | None => return RetryAction::NoActionIndicated,
            Some(Err(err)) => err,
        };

        let action = if let Some(err) = error.as_operation_error() {
            tracing::info!("its an operation error: {err:?}");
            let err = err.downcast_ref::<OperationError>().unwrap();
            RetryAction::retryable_error(err.0)
        } else {
            tracing::info!("its something else... using other classifiers");
            let action = TransientErrorClassifier::<OperationError>::new().classify_retry(ctx);
            if action == RetryAction::NoActionIndicated {
                HttpStatusCodeClassifier::default().classify_retry(ctx)
            } else {
                action
            }
        };

        tracing::info!("classified as {action:?}");
        action
    }

    fn name(&self) -> &'static str {
        "test"
    }
}

/// MakeClient — parameterizes tests over HTTP stacks
trait MakeClient: Send + Sync {
    fn make(&self, mock: &WireMockServer) -> SharedHttpClient;
}

/// hyper 0.14 legacy stack
struct Hyper014Client;

impl MakeClient for Hyper014Client {
    fn make(&self, mock: &WireMockServer) -> SharedHttpClient {
        HyperClientBuilder::new()
            .build(hyper_0_14::client::HttpConnector::new_with_resolver(
                mock.dns_resolver(),
            ))
            .into_shared()
    }
}

/// hyper 0.14 with HTTP/2 only
struct Hyper014H2Client;

impl MakeClient for Hyper014H2Client {
    fn make(&self, mock: &WireMockServer) -> SharedHttpClient {
        let mut hyper_builder = hyper_0_14::Client::builder();
        hyper_builder.http2_only(true);
        HyperClientBuilder::new()
            .hyper_builder(hyper_builder)
            .build(hyper_0_14::client::HttpConnector::new_with_resolver(
                mock.dns_resolver(),
            ))
            .into_shared()
    }
}

/// hyper 1.x stack via public Builder API
struct Hyper1xClient;

impl MakeClient for Hyper1xClient {
    fn make(&self, mock: &WireMockServer) -> SharedHttpClient {
        aws_smithy_http_client::Builder::new().build_with_resolver(mock.dns_resolver())
    }
}

/// v2 HTTP client (composable pool) via `BuilderV2::new_v2()`
struct Hyper1xV2Client;

impl MakeClient for Hyper1xV2Client {
    fn make(&self, mock: &WireMockServer) -> SharedHttpClient {
        aws_smithy_http_client::v2::BuilderV2::new().build_http_with_resolver(mock.dns_resolver())
    }
}

/// Repeatedly send test operation until `end_of_test` is received, then run match_clause.
async fn run_test(
    make_client: &dyn MakeClient,
    events: Vec<ReplayedEvent>,
    reconnect_mode: ReconnectMode,
    match_clause: impl Fn(&[RecordedEvent]),
) {
    let mock = WireMockServer::start(events).await;
    let http_client = make_client.make(&mock);

    let operation = Operation::builder()
        .service_name("test")
        .operation_name("test")
        .no_auth()
        .endpoint_url(&mock.endpoint_url())
        .http_client(http_client)
        .timeout_config(
            TimeoutConfig::builder()
                .operation_attempt_timeout(Duration::from_millis(100))
                .build(),
        )
        .standard_retry(&RetryConfig::standard().with_reconnect_mode(reconnect_mode))
        .retry_classifier(TestRetryClassifier)
        .sleep_impl(TokioSleep::new())
        .with_connection_poisoning()
        .serializer({
            let endpoint_url = mock.endpoint_url();
            move |_| {
                let request = http_1x::Request::builder()
                    .uri(endpoint_url.clone())
                    .body(SdkBody::from("body"))
                    .unwrap()
                    .try_into()
                    .unwrap();
                tracing::info!("serializing request: {request:?}");
                Ok(request)
            }
        })
        .deserializer(|response| {
            tracing::info!("deserializing response: {:?}", response);
            match response.status() {
                s if s.is_success() => {
                    Ok(String::from_utf8(response.body().bytes().unwrap().into()).unwrap())
                }
                s if s.is_client_error() => Err(OrchestratorError::operation(OperationError(
                    ErrorKind::ServerError,
                ))),
                s if s.is_server_error() => Err(OrchestratorError::operation(OperationError(
                    ErrorKind::TransientError,
                ))),
                _ => panic!("unexpected status: {}", response.status()),
            }
        })
        .build();

    let mut iteration = 0;
    loop {
        tracing::info!("iteration {iteration}...");
        match operation.invoke(()).await {
            Ok(resp) => {
                tracing::info!("response: {:?}", resp);
                if resp == END_OF_TEST {
                    break;
                }
            }
            Err(e) => tracing::info!("error: {:?}", e),
        }
        iteration += 1;
        if iteration > 50 {
            panic!("probably an infinite loop; no satisfying 'end_of_test' response received");
        }
    }
    let events = mock.events();
    match_clause(&events);
    mock.shutdown();
}

/// Run a test against all HTTP stacks
async fn all_stacks(
    events: Vec<ReplayedEvent>,
    reconnect_mode: ReconnectMode,
    match_clause: impl Fn(&[RecordedEvent]),
) {
    run_test(
        &Hyper014Client,
        events.clone(),
        reconnect_mode,
        &match_clause,
    )
    .await;
    run_test(
        &Hyper014H2Client,
        events.clone(),
        reconnect_mode,
        &match_clause,
    )
    .await;
    run_test(
        &Hyper1xClient,
        events.clone(),
        reconnect_mode,
        &match_clause,
    )
    .await;
    run_test(&Hyper1xV2Client, events, reconnect_mode, &match_clause).await;
}

#[tokio::test]
async fn non_transient_errors_no_reconnect() {
    let _logs = capture_test_logs();
    all_stacks(
        vec![
            ReplayedEvent::status(400),
            ReplayedEvent::with_body(END_OF_TEST),
        ],
        ReconnectMode::ReconnectOnTransientError,
        match_events!(ev!(dns), ev!(connect), ev!(http(400)), ev!(http(200))),
    )
    .await;
}

#[tokio::test]
async fn reestablish_dns_on_503() {
    let _logs = capture_test_logs();
    all_stacks(
        vec![
            ReplayedEvent::status(503),
            ReplayedEvent::status(503),
            ReplayedEvent::status(503),
            ReplayedEvent::with_body(END_OF_TEST),
        ],
        ReconnectMode::ReconnectOnTransientError,
        match_events!(
            // first request
            ev!(dns),
            ev!(connect),
            ev!(http(503)),
            // second request
            ev!(dns),
            ev!(connect),
            ev!(http(503)),
            // third request
            ev!(dns),
            ev!(connect),
            ev!(http(503)),
            // all good
            ev!(dns),
            ev!(connect),
            ev!(http(200))
        ),
    )
    .await;
}

#[tokio::test]
async fn connection_shared_on_success() {
    let _logs = capture_test_logs();
    all_stacks(
        vec![
            ReplayedEvent::ok(),
            ReplayedEvent::ok(),
            ReplayedEvent::status(503),
            ReplayedEvent::with_body(END_OF_TEST),
        ],
        ReconnectMode::ReconnectOnTransientError,
        match_events!(
            ev!(dns),
            ev!(connect),
            ev!(http(200)),
            ev!(http(200)),
            ev!(http(503)),
            ev!(dns),
            ev!(connect),
            ev!(http(200))
        ),
    )
    .await;
}

#[tokio::test]
async fn no_reconnect_when_disabled() {
    let _logs = capture_test_logs();
    all_stacks(
        vec![
            ReplayedEvent::status(503),
            ReplayedEvent::with_body(END_OF_TEST),
        ],
        ReconnectMode::ReuseAllConnections,
        match_events!(ev!(dns), ev!(connect), ev!(http(503)), ev!(http(200))),
    )
    .await;
}

#[tokio::test]
async fn connection_reestablished_after_timeout() {
    let _logs = capture_test_logs();
    all_stacks(
        vec![
            ReplayedEvent::ok(),
            ReplayedEvent::Timeout,
            ReplayedEvent::ok(),
            ReplayedEvent::Timeout,
            ReplayedEvent::with_body(END_OF_TEST),
        ],
        ReconnectMode::ReconnectOnTransientError,
        match_events!(
            // first connection
            ev!(dns),
            ev!(connect),
            ev!(http(200)),
            // reuse but got a timeout
            ev!(timeout),
            // so we reconnect
            ev!(dns),
            ev!(connect),
            ev!(http(200)),
            ev!(timeout),
            ev!(dns),
            ev!(connect),
            ev!(http(200))
        ),
    )
    .await;
}
