/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Drives the real orchestrator through `Operation` and checks the emitted entries.

use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use aws_smithy_client_metrics::{ClientMetrics, Destination, MissingPoolBehavior};
use aws_smithy_http_client::test_util::infallible_client_fn;
use aws_smithy_runtime::client::orchestrator::operation::Operation;
use aws_smithy_runtime::client::retries::classifiers::HttpStatusCodeClassifier;
use aws_smithy_runtime_api::client::connection::ConnectionMetadata;
use aws_smithy_runtime_api::client::http::telemetry::{
    CaptureHttpAttemptTelemetry, ConnectionAcquisitionTelemetry, ConnectionUsage,
};
use aws_smithy_runtime_api::client::http::SharedHttpClient;
use aws_smithy_runtime_api::client::interceptors::context::{Error, InterceptorContext, Output};
use aws_smithy_runtime_api::client::interceptors::Intercept;
use aws_smithy_runtime_api::client::orchestrator::{HttpRequest, HttpResponse, OrchestratorError};
use aws_smithy_runtime_api::client::result::ConnectorError;
use aws_smithy_runtime_api::client::retries::classifiers::{ClassifyRetry, RetryAction};
use aws_smithy_runtime_api::client::ser_de::DeserializeResponse;
use aws_smithy_types::body::SdkBody;
use aws_smithy_types::retry::RetryConfig;
use aws_smithy_types::timeout::TimeoutConfig;
use metrique::test_util::{test_entry_sink, test_metric, TestEntry};
use metrique::unit_of_work::metrics;
use metrique::ServiceMetrics;
use metrique_util::{with_metrics_pool, MetricsPool};

#[metrics(rename_all = "PascalCase")]
struct RequestMetrics {
    route: &'static str,
    #[metrics(flatten)]
    sdk: MetricsPool,
}

fn request() -> RequestMetrics {
    RequestMetrics {
        route: "/test",
        sdk: MetricsPool::new(),
    }
}

/// Classifies 429 as throttling, the way AWS classifiers do.
#[derive(Debug)]
struct ThrottlingClassifier;

impl ClassifyRetry for ThrottlingClassifier {
    fn classify_retry(&self, ctx: &InterceptorContext) -> RetryAction {
        match ctx.response().map(|r| r.status().as_u16()) {
            Some(429) => RetryAction::throttling_error(),
            _ => RetryAction::NoActionIndicated,
        }
    }

    fn name(&self) -> &'static str {
        "ThrottlingClassifier"
    }
}

#[derive(Debug)]
struct Deserializer;

impl DeserializeResponse for Deserializer {
    fn deserialize_nonstreaming(
        &self,
        resp: &HttpResponse,
    ) -> Result<Output, OrchestratorError<Error>> {
        if resp.status().is_success() {
            Ok(Output::erase("output".to_owned()))
        } else {
            Err(OrchestratorError::connector(ConnectorError::io(
                "error response".into(),
            )))
        }
    }
}

/// An HTTP client that returns `statuses` in order and reports connection
/// telemetry: the first attempt uses a fresh connection, later ones reuse it.
fn http_client(statuses: &[u16]) -> SharedHttpClient {
    let statuses = statuses.to_vec();
    let calls = Arc::new(AtomicUsize::new(0));
    infallible_client_fn(move |req| {
        let n = calls.fetch_add(1, Ordering::SeqCst);
        if let Some(capture) = req.extensions().get::<CaptureHttpAttemptTelemetry>() {
            let usage = if n == 0 {
                ConnectionUsage::Fresh
            } else {
                ConnectionUsage::Reused
            };
            capture.record_connection_selection(
                ConnectionAcquisitionTelemetry::new(Duration::from_millis(1), usage),
                ConnectionMetadata::builder()
                    .proxied(false)
                    .poison_fn(|| {})
                    .build(),
            );
            capture.record_connector_call_duration(Duration::from_millis(2));
        }
        let status = statuses[n.min(statuses.len() - 1)];
        http_1x::Response::builder()
            .status(status)
            .body(SdkBody::empty())
            .unwrap()
    })
}

fn operation(
    service: &'static str,
    name: &'static str,
    statuses: &[u16],
    interceptors: Vec<Box<dyn FnOnce(OpBuilder) -> OpBuilder>>,
) -> Operation<(), String, Infallible> {
    let mut builder = Operation::builder()
        .service_name(service)
        .operation_name(name)
        .http_client(http_client(statuses))
        .endpoint_url("http://localhost:1234/doesntmatter")
        .no_auth()
        .retry_classifier(HttpStatusCodeClassifier::default())
        .retry_classifier(ThrottlingClassifier)
        .standard_retry(
            &RetryConfig::standard()
                .with_max_attempts(3)
                .with_initial_backoff(Duration::from_millis(1))
                .with_max_backoff(Duration::from_millis(1)),
        )
        .timeout_config(TimeoutConfig::disabled())
        .serializer(|_: ()| Ok(HttpRequest::new(SdkBody::empty())))
        .deserializer_impl(Deserializer);
    for f in interceptors {
        builder = f(builder);
    }
    builder.build()
}

type OpBuilder =
    aws_smithy_runtime::client::orchestrator::operation::OperationBuilder<(), String, Infallible>;

fn with(interceptor: impl Intercept + Clone + 'static) -> Box<dyn FnOnce(OpBuilder) -> OpBuilder> {
    Box::new(move |b| b.interceptor(interceptor))
}

fn s3_get(statuses: &[u16], metrics: ClientMetrics) -> Operation<(), String, Infallible> {
    operation("S3", "GetObject", statuses, vec![with(metrics)])
}

fn keys(entry: &TestEntry) -> Vec<String> {
    let mut keys: Vec<String> = entry
        .values
        .keys()
        .chain(entry.metrics.keys())
        .cloned()
        .collect();
    keys.sort();
    keys
}

fn sdk_keys(entry: &TestEntry) -> Vec<String> {
    keys(entry)
        .into_iter()
        .filter(|k| k.starts_with("Sdk"))
        .collect()
}

#[tokio::test]
async fn pooled_call_adds_fields_to_the_request_entry() {
    let req = request();
    let op = s3_get(&[200], ClientMetrics::new());
    with_metrics_pool(req.sdk.handle(), op.invoke(()))
        .await
        .unwrap();

    let entry = test_metric(req);
    assert_eq!(entry.values["Route"], "/test");
    assert_eq!(entry.values["SdkS3GetObjectHttpStatusCode"], "200");
    assert_eq!(entry.metrics["SdkS3GetObjectApiCallSuccessful"].as_u64(), 1);
    assert_eq!(entry.metrics["SdkS3GetObjectAttempts"].as_u64(), 1);
    assert_eq!(entry.metrics["SdkS3GetObjectRetryCount"].as_u64(), 0);
    assert_eq!(entry.metrics["SdkS3GetObjectThrottles"].as_u64(), 0);
    assert_eq!(entry.metrics["SdkS3GetObjectConnectionReused"].as_u64(), 0);
}

/// The emitted names are the public contract. Changing them breaks dashboards.
#[tokio::test]
async fn pooled_key_names_are_stable() {
    let req = request();
    with_metrics_pool(
        req.sdk.handle(),
        s3_get(&[200], ClientMetrics::new()).invoke(()),
    )
    .await
    .unwrap();

    assert_eq!(
        sdk_keys(&test_metric(req)),
        [
            "SdkS3GetObjectApiCallDuration",
            "SdkS3GetObjectApiCallSuccessful",
            "SdkS3GetObjectAttemptTimeouts",
            "SdkS3GetObjectAttempts",
            "SdkS3GetObjectBackoffDelayDuration",
            "SdkS3GetObjectCancelled",
            "SdkS3GetObjectClientError",
            "SdkS3GetObjectConcurrencyAcquireDuration",
            "SdkS3GetObjectConnectionReused",
            "SdkS3GetObjectHttpStatusCode",
            "SdkS3GetObjectOperationTimeout",
            "SdkS3GetObjectRetryCount",
            "SdkS3GetObjectServerError",
            "SdkS3GetObjectServiceCallDuration",
            "SdkS3GetObjectThrottles",
        ]
    );
}

#[tokio::test]
async fn standalone_key_names_are_stable() {
    let sink = test_entry_sink();
    let metrics = ClientMetrics::builder()
        .standalone()
        .standalone_sink(sink.sink)
        .build();
    s3_get(&[200], metrics).invoke(()).await.unwrap();

    let entries = sink.inspector.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        keys(&entries[0]),
        [
            "ApiCallDuration",
            "ApiCallSuccessful",
            "AttemptTimeouts",
            "Attempts",
            "BackoffDelayDuration",
            "Cancelled",
            "ClientError",
            "ConcurrencyAcquireDuration",
            "ConnectionReused",
            "HttpStatusCode",
            "OperationName",
            "OperationTimeout",
            "RetryCount",
            "ServerError",
            "ServiceCallDuration",
            "ServiceId",
            "Throttles",
        ]
    );
}

/// A fallback entry has the standalone keys plus `MetricsPoolFallback`.
#[tokio::test]
async fn fallback_key_names_are_stable() {
    let sink = test_entry_sink();
    let metrics = ClientMetrics::builder().standalone_sink(sink.sink).build();
    s3_get(&[200], metrics).invoke(()).await.unwrap();

    let entries = sink.inspector.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        keys(&entries[0]),
        [
            "ApiCallDuration",
            "ApiCallSuccessful",
            "AttemptTimeouts",
            "Attempts",
            "BackoffDelayDuration",
            "Cancelled",
            "ClientError",
            "ConcurrencyAcquireDuration",
            "ConnectionReused",
            "HttpStatusCode",
            "MetricsPoolFallback",
            "OperationName",
            "OperationTimeout",
            "RetryCount",
            "ServerError",
            "ServiceCallDuration",
            "ServiceId",
            "Throttles",
        ]
    );
}

#[tokio::test]
async fn retries_and_throttles_are_counted_across_attempts() {
    let req = request();
    let op = s3_get(&[429, 503, 200], ClientMetrics::new());
    with_metrics_pool(req.sdk.handle(), op.invoke(()))
        .await
        .unwrap();

    let entry = test_metric(req);
    assert_eq!(entry.metrics["SdkS3GetObjectAttempts"].as_u64(), 3);
    assert_eq!(entry.metrics["SdkS3GetObjectRetryCount"].as_u64(), 2);
    assert_eq!(entry.metrics["SdkS3GetObjectThrottles"].as_u64(), 1);
    assert_eq!(entry.metrics["SdkS3GetObjectApiCallSuccessful"].as_u64(), 1);
    // The final attempt reused the connection opened by the first.
    assert_eq!(entry.metrics["SdkS3GetObjectConnectionReused"].as_u64(), 1);
}

#[tokio::test]
async fn failures_report_the_final_status() {
    let req = request();
    let op = s3_get(&[404], ClientMetrics::new());
    with_metrics_pool(req.sdk.handle(), op.invoke(()))
        .await
        .unwrap_err();

    let entry = test_metric(req);
    assert_eq!(entry.values["SdkS3GetObjectHttpStatusCode"], "404");
    assert_eq!(entry.metrics["SdkS3GetObjectApiCallSuccessful"].as_u64(), 0);
    assert_eq!(entry.metrics["SdkS3GetObjectClientError"].as_u64(), 1);
    assert_eq!(entry.metrics["SdkS3GetObjectServerError"].as_u64(), 0);
    assert_eq!(entry.metrics["SdkS3GetObjectAttempts"].as_u64(), 1);
}

#[tokio::test]
async fn different_operations_do_not_collide_in_one_pool() {
    let req = request();
    let metrics = ClientMetrics::new();
    with_metrics_pool(req.sdk.handle(), async {
        operation("DynamoDB", "GetItem", &[200], vec![with(metrics.clone())])
            .invoke(())
            .await
            .unwrap();
        operation(
            "DynamoDB",
            "PutItem",
            &[500, 200],
            vec![with(metrics.clone())],
        )
        .invoke(())
        .await
        .unwrap();
    })
    .await;

    let entry = test_metric(req);
    assert_eq!(entry.metrics["SdkDynamoDBGetItemRetryCount"].as_u64(), 0);
    assert_eq!(entry.metrics["SdkDynamoDBPutItemRetryCount"].as_u64(), 1);
}

#[tokio::test]
async fn repeated_calls_to_the_same_operation_keep_the_last_call() {
    let req = request();
    let metrics = ClientMetrics::new();
    with_metrics_pool(req.sdk.handle(), async {
        s3_get(&[503, 200], metrics.clone())
            .invoke(())
            .await
            .unwrap();
        s3_get(&[200], metrics).invoke(()).await.unwrap();
    })
    .await;

    let entry = test_metric(req);
    assert_eq!(entry.metrics["SdkS3GetObjectAttempts"].as_u64(), 1);
    assert_eq!(entry.metrics["SdkS3GetObjectRetryCount"].as_u64(), 0);
}

#[tokio::test]
async fn service_names_are_normalized() {
    let req = request();
    let op = operation(
        "CloudWatch Logs",
        "PutLogEvents",
        &[200],
        vec![with(ClientMetrics::new())],
    );
    with_metrics_pool(req.sdk.handle(), op.invoke(()))
        .await
        .unwrap();
    let entry = test_metric(req);
    assert_eq!(
        entry.metrics["SdkCloudWatchLogsPutLogEventsApiCallSuccessful"].as_u64(),
        1
    );
}

#[tokio::test]
async fn qualifier_is_part_of_the_prefix() {
    let req = request();
    let op = s3_get(&[200], ClientMetrics::builder().qualifier("Audit").build());
    with_metrics_pool(req.sdk.handle(), op.invoke(()))
        .await
        .unwrap();
    let entry = test_metric(req);
    assert_eq!(
        entry.metrics["SdkS3AuditGetObjectApiCallSuccessful"].as_u64(),
        1
    );
}

#[tokio::test]
async fn prefix_replaces_everything_before_the_operation() {
    let req = request();
    let op = s3_get(
        &[200],
        ClientMetrics::builder()
            .qualifier("Audit")
            .prefix("AuditLog")
            .build(),
    );
    with_metrics_pool(req.sdk.handle(), op.invoke(()))
        .await
        .unwrap();
    let entry = test_metric(req);
    assert_eq!(
        entry.metrics["AuditLogGetObjectApiCallSuccessful"].as_u64(),
        1
    );
    assert!(
        sdk_keys(&entry).is_empty(),
        "no Sdk-prefixed keys expected: {:?}",
        sdk_keys(&entry)
    );
}

#[tokio::test]
async fn prefix_does_not_affect_standalone_entries() {
    let sink = test_entry_sink();
    let op = s3_get(
        &[200],
        ClientMetrics::builder()
            .qualifier("Audit")
            .prefix("AuditLog")
            .standalone()
            .standalone_sink(sink.sink)
            .build(),
    );
    op.invoke(()).await.unwrap();
    let entries = sink.inspector.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].values["Qualifier"], "Audit");
    assert_eq!(entries[0].metrics["ApiCallSuccessful"].as_u64(), 1);
    assert!(!keys(&entries[0]).iter().any(|k| k.starts_with("AuditLog")));
}

#[tokio::test]
async fn missing_pool_falls_back_to_a_flagged_standalone_entry() {
    let sink = test_entry_sink();
    let op = s3_get(
        &[200],
        ClientMetrics::builder().standalone_sink(sink.sink).build(),
    );
    op.invoke(()).await.unwrap();

    let entries = sink.inspector.entries();
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.values["ServiceId"], "S3");
    assert_eq!(entry.values["OperationName"], "GetObject");
    assert_eq!(entry.metrics["MetricsPoolFallback"].as_u64(), 1);
    assert_eq!(entry.metrics["ApiCallSuccessful"].as_u64(), 1);
    assert!(entry.timestamp.is_some());
}

#[tokio::test]
async fn emit_standalone_behavior_writes_a_flagged_entry() {
    let sink = test_entry_sink();
    let metrics = ClientMetrics::builder()
        .destination(Destination::CurrentPool)
        .on_missing_pool(MissingPoolBehavior::EmitStandalone)
        .standalone_sink(sink.sink)
        .build();
    s3_get(&[200], metrics).invoke(()).await.unwrap();

    let entries = sink.inspector.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].metrics["MetricsPoolFallback"].as_u64(), 1);
}

#[tokio::test]
async fn require_current_pool_drops_metrics_when_the_pool_is_missing() {
    let sink = test_entry_sink();
    let metrics = ClientMetrics::builder()
        .require_current_pool()
        .standalone_sink(sink.sink)
        .build();
    s3_get(&[200], metrics).invoke(()).await.unwrap();

    assert!(sink.inspector.entries().is_empty());
}

#[tokio::test]
async fn require_current_pool_emits_when_a_pool_is_in_scope() {
    let req = request();
    let metrics = ClientMetrics::builder().require_current_pool().build();
    with_metrics_pool(req.sdk.handle(), s3_get(&[200], metrics).invoke(()))
        .await
        .unwrap();

    assert_eq!(
        test_metric(req).metrics["SdkS3GetObjectApiCallSuccessful"].as_u64(),
        1
    );
}

#[tokio::test]
async fn missing_pool_behavior_does_not_affect_a_standalone_destination() {
    let sink = test_entry_sink();
    let metrics = ClientMetrics::builder()
        .destination(Destination::Standalone)
        .on_missing_pool(MissingPoolBehavior::Drop)
        .standalone_sink(sink.sink)
        .build();
    s3_get(&[200], metrics).invoke(()).await.unwrap();

    assert_eq!(sink.inspector.entries().len(), 1);
}

#[tokio::test]
async fn standalone_mode_ignores_the_pool_and_is_not_flagged() {
    let sink = test_entry_sink();
    let req = request();
    let metrics = ClientMetrics::builder()
        .standalone()
        .standalone_sink(sink.sink)
        .build();
    with_metrics_pool(req.sdk.handle(), s3_get(&[200], metrics).invoke(()))
        .await
        .unwrap();

    assert!(sdk_keys(&test_metric(req)).is_empty());
    let entries = sink.inspector.entries();
    assert_eq!(entries.len(), 1);
    assert!(!entries[0].metrics.contains_key("MetricsPoolFallback"));
}

#[tokio::test]
async fn standalone_entries_default_to_the_global_sink() {
    let sink = test_entry_sink();
    let _guard = ServiceMetrics::set_test_sink(sink.sink);
    s3_get(&[200], ClientMetrics::new())
        .invoke(())
        .await
        .unwrap();
    assert_eq!(sink.inspector.entries().len(), 1);
}

#[tokio::test]
async fn explicit_pool_needs_no_scope() {
    let req = request();
    let metrics = ClientMetrics::builder()
        .metrics_pool(req.sdk.handle())
        .build();
    s3_get(&[200], metrics).invoke(()).await.unwrap();
    let entry = test_metric(req);
    assert_eq!(entry.metrics["SdkS3GetObjectApiCallSuccessful"].as_u64(), 1);
}

#[tokio::test]
async fn a_second_client_metrics_does_not_double_record() {
    let first = test_entry_sink();
    let second = test_entry_sink();
    let op = operation(
        "S3",
        "GetObject",
        &[200],
        vec![
            with(
                ClientMetrics::builder()
                    .standalone()
                    .standalone_sink(first.sink)
                    .build(),
            ),
            with(
                ClientMetrics::builder()
                    .standalone()
                    .standalone_sink(second.sink)
                    .build(),
            ),
        ],
    );
    op.invoke(()).await.unwrap();
    assert_eq!(
        first.inspector.entries().len() + second.inspector.entries().len(),
        1
    );
}

#[tokio::test]
async fn two_registrations_of_the_same_client_metrics_are_idempotent() {
    let sink = test_entry_sink();
    let metrics = ClientMetrics::builder()
        .standalone()
        .standalone_sink(sink.sink)
        .build();
    let op = operation(
        "S3",
        "GetObject",
        &[503, 200],
        vec![with(metrics.clone()), with(metrics)],
    );

    op.invoke(()).await.unwrap();

    let entries = sink.inspector.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].metrics["Attempts"].as_u64(), 2);
    assert_eq!(entries[0].metrics["RetryCount"].as_u64(), 1);
    assert_eq!(entries[0].metrics["AttemptTimeouts"].as_u64(), 0);
}

#[tokio::test]
async fn one_client_metrics_records_every_call() {
    let sink = test_entry_sink();
    let metrics = ClientMetrics::builder()
        .standalone()
        .standalone_sink(sink.sink)
        .build();
    for _ in 0..3 {
        s3_get(&[200], metrics.clone()).invoke(()).await.unwrap();
    }
    assert_eq!(sink.inspector.entries().len(), 3);
}

#[tokio::test]
async fn installs_as_a_runtime_plugin() {
    let sink = test_entry_sink();
    let metrics = ClientMetrics::builder()
        .standalone()
        .standalone_sink(sink.sink)
        .build();
    let op = operation(
        "S3",
        "GetObject",
        &[503, 200],
        vec![Box::new(move |b| b.runtime_plugin(metrics))],
    );
    op.invoke(()).await.unwrap();

    let entries = sink.inspector.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].metrics["RetryCount"].as_u64(), 1);
}

#[tokio::test]
async fn durations_use_the_client_time_source() {
    use aws_smithy_async::test_util::ManualTimeSource;
    use std::time::UNIX_EPOCH;

    let time = ManualTimeSource::new(UNIX_EPOCH + Duration::from_secs(1_000));
    let clock = time.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    // Each attempt takes 10ms on the client's clock; the first fails with 503.
    let http = infallible_client_fn(move |_req| {
        clock.advance(Duration::from_millis(10));
        let status = if calls.fetch_add(1, Ordering::SeqCst) == 0 {
            503
        } else {
            200
        };
        http_1x::Response::builder()
            .status(status)
            .body(SdkBody::empty())
            .unwrap()
    });

    let req = request();
    let metrics = ClientMetrics::new();
    let ts = time.clone();
    let op = operation(
        "S3",
        "GetObject",
        &[200],
        vec![
            with(metrics),
            Box::new(move |b| b.http_client(http).time_source(ts)),
        ],
    );
    with_metrics_pool(req.sdk.handle(), op.invoke(()))
        .await
        .unwrap();

    let entry = test_metric(req);
    // Two attempts of 10ms each. Backoff sleeps don't advance the manual clock.
    assert_eq!(
        entry.metrics["SdkS3GetObjectApiCallDuration"].as_f64(),
        20.0
    );
    assert_eq!(entry.metrics["SdkS3GetObjectRetryCount"].as_u64(), 1);
}

#[tokio::test]
async fn successful_calls_have_no_error_type() {
    let req = request();
    with_metrics_pool(
        req.sdk.handle(),
        s3_get(&[200], ClientMetrics::new()).invoke(()),
    )
    .await
    .unwrap();
    let entry = test_metric(req);
    assert!(!entry.values.contains_key("SdkS3GetObjectErrorType"));
    assert_eq!(
        entry.metrics["SdkS3GetObjectBackoffDelayDuration"].as_f64(),
        0.0
    );
    assert_eq!(
        entry.metrics["SdkS3GetObjectConcurrencyAcquireDuration"].as_f64(),
        1.0
    );
}

async fn error_type_for(statuses: &[u16]) -> String {
    let req = request();
    with_metrics_pool(
        req.sdk.handle(),
        s3_get(statuses, ClientMetrics::new()).invoke(()),
    )
    .await
    .unwrap_err();
    test_metric(req).values["SdkS3GetObjectErrorType"].clone()
}

#[tokio::test]
async fn throttling_error_type() {
    assert_eq!(error_type_for(&[429]).await, "Throttling");
}

#[tokio::test]
async fn server_error_type_for_4xx_and_5xx() {
    assert_eq!(error_type_for(&[404]).await, "ServerError");
    assert_eq!(error_type_for(&[500]).await, "ServerError");
}

#[tokio::test]
async fn io_error_type() {
    let req = request();
    let op = operation(
        "S3",
        "GetObject",
        &[200],
        vec![
            with(ClientMetrics::new()),
            Box::new(|b| b.http_client(IoErrorClient).no_retry()),
        ],
    );
    with_metrics_pool(req.sdk.handle(), op.invoke(()))
        .await
        .unwrap_err();
    let entry = test_metric(req);
    assert_eq!(entry.values["SdkS3GetObjectErrorType"], "IO");
    assert_eq!(entry.metrics["SdkS3GetObjectAttempts"].as_u64(), 1);
}

#[tokio::test]
async fn failure_before_any_attempt_is_other_with_zero_attempts() {
    let req = request();
    let op = operation(
        "S3",
        "GetObject",
        &[200],
        vec![
            with(ClientMetrics::new()),
            Box::new(|b| b.serializer(|_: ()| Err("cannot serialize".into()))),
        ],
    );
    with_metrics_pool(req.sdk.handle(), op.invoke(()))
        .await
        .unwrap_err();
    let entry = test_metric(req);
    assert_eq!(entry.values["SdkS3GetObjectErrorType"], "Other");
    assert_eq!(entry.metrics["SdkS3GetObjectAttempts"].as_u64(), 0);
    assert!(!entry
        .metrics
        .contains_key("SdkS3GetObjectBackoffDelayDuration"));
}

#[tokio::test]
async fn backoff_delay_duration_matches_time_slept_between_attempts() {
    use aws_smithy_async::test_util::instant_time_and_sleep;
    use std::time::UNIX_EPOCH;

    let (time, sleep) = instant_time_and_sleep(UNIX_EPOCH + Duration::from_secs(1_000));
    let req = request();
    let slept = sleep.clone();
    let op = operation(
        "S3",
        "GetObject",
        &[503, 503, 200],
        vec![
            with(ClientMetrics::new()),
            Box::new(move |b| {
                b.time_source(time).sleep_impl(slept).standard_retry(
                    &RetryConfig::standard()
                        .with_max_attempts(3)
                        .with_initial_backoff(Duration::from_millis(100))
                        .with_max_backoff(Duration::from_secs(1)),
                )
            }),
        ],
    );
    with_metrics_pool(req.sdk.handle(), op.invoke(()))
        .await
        .unwrap();

    let entry = test_metric(req);
    let expected_ms = sleep.total_duration().as_secs_f64() * 1000.0;
    let actual_ms = entry.metrics["SdkS3GetObjectBackoffDelayDuration"].as_f64();
    assert_eq!(sleep.logs().len(), 2, "one backoff per retry");
    assert!(expected_ms > 0.0);
    assert!(
        (actual_ms - expected_ms).abs() < 1e-6,
        "BackoffDelayDuration {actual_ms}ms != slept {expected_ms}ms"
    );
}

/// Fails every request with an I/O connector error.
#[derive(Clone, Debug)]
struct IoErrorClient;

impl aws_smithy_runtime_api::client::http::HttpConnector for IoErrorClient {
    fn call(
        &self,
        _request: HttpRequest,
    ) -> aws_smithy_runtime_api::client::http::HttpConnectorFuture {
        aws_smithy_runtime_api::client::http::HttpConnectorFuture::ready(Err(ConnectorError::io(
            "connection reset".into(),
        )))
    }
}

impl aws_smithy_runtime_api::client::http::HttpClient for IoErrorClient {
    fn http_connector(
        &self,
        _settings: &aws_smithy_runtime_api::client::http::HttpConnectorSettings,
        _components: &aws_smithy_runtime_api::client::runtime_components::RuntimeComponents,
    ) -> aws_smithy_runtime_api::client::http::SharedHttpConnector {
        aws_smithy_runtime_api::client::http::SharedHttpConnector::new(self.clone())
    }
}

mod timeouts {
    use super::*;
    use aws_smithy_async::rt::sleep::TokioSleep;
    use aws_smithy_http_client::test_util::NeverClient;
    use aws_smithy_runtime::client::retries::classifiers::TransientErrorClassifier;
    use aws_smithy_runtime_api::client::http::{
        HttpClient, HttpConnector, HttpConnectorFuture, HttpConnectorSettings, SharedHttpConnector,
    };
    use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;

    /// Responds 200, but the first `slow_calls` calls take `delay` to respond.
    #[derive(Clone, Debug)]
    struct SlowFirstCalls {
        slow_calls: usize,
        delay: Duration,
        calls: Arc<AtomicUsize>,
    }

    impl SlowFirstCalls {
        fn new(slow_calls: usize, delay: Duration) -> Self {
            Self {
                slow_calls,
                delay,
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl HttpConnector for SlowFirstCalls {
        fn call(&self, _request: HttpRequest) -> HttpConnectorFuture {
            let slow = self.calls.fetch_add(1, Ordering::SeqCst) < self.slow_calls;
            let delay = self.delay;
            HttpConnectorFuture::new(async move {
                if slow {
                    tokio::time::sleep(delay).await;
                }
                Ok(HttpResponse::try_from(
                    http_1x::Response::builder()
                        .status(200)
                        .body(SdkBody::empty())
                        .unwrap(),
                )
                .unwrap())
            })
        }
    }

    impl HttpClient for SlowFirstCalls {
        fn http_connector(
            &self,
            _settings: &HttpConnectorSettings,
            _components: &RuntimeComponents,
        ) -> SharedHttpConnector {
            SharedHttpConnector::new(self.clone())
        }
    }

    /// Records a reused-connection selection, then never responds: the attempt
    /// stalls after the connection was chosen.
    #[derive(Clone, Debug)]
    struct StallsAfterConnecting;

    impl HttpConnector for StallsAfterConnecting {
        fn call(&self, request: HttpRequest) -> HttpConnectorFuture {
            if let Some(capture) = request.extension::<CaptureHttpAttemptTelemetry>() {
                capture.record_connection_selection(
                    ConnectionAcquisitionTelemetry::new(
                        Duration::from_millis(1),
                        ConnectionUsage::Reused,
                    ),
                    ConnectionMetadata::builder()
                        .proxied(false)
                        .poison_fn(|| {})
                        .build(),
                );
            }
            HttpConnectorFuture::new(std::future::pending())
        }
    }

    impl HttpClient for StallsAfterConnecting {
        fn http_connector(
            &self,
            _settings: &HttpConnectorSettings,
            _components: &RuntimeComponents,
        ) -> SharedHttpConnector {
            SharedHttpConnector::new(self.clone())
        }
    }

    const SHORT: Duration = Duration::from_millis(50);
    const LONG: Duration = Duration::from_secs(5);

    fn timed_op(
        http: impl HttpClient + 'static,
        timeouts: TimeoutConfig,
        max_attempts: u32,
        metrics: ClientMetrics,
    ) -> Operation<(), String, Infallible> {
        operation(
            "S3",
            "GetObject",
            &[200],
            vec![
                with(metrics),
                Box::new(move |b| {
                    b.http_client(http)
                        .sleep_impl(TokioSleep::new())
                        .timeout_config(timeouts)
                        .retry_classifier(TransientErrorClassifier::<Infallible>::new())
                        .standard_retry(
                            &RetryConfig::standard()
                                .with_max_attempts(max_attempts)
                                .with_initial_backoff(Duration::from_millis(1))
                                .with_max_backoff(Duration::from_millis(1)),
                        )
                }),
            ],
        )
    }

    #[tokio::test]
    async fn attempt_timeout_then_retry_is_recorded() {
        let req = request();
        let op = timed_op(
            SlowFirstCalls::new(1, LONG),
            TimeoutConfig::builder()
                .operation_attempt_timeout(SHORT)
                .build(),
            3,
            ClientMetrics::new(),
        );
        with_metrics_pool(req.sdk.handle(), op.invoke(()))
            .await
            .unwrap();

        let entry = test_metric(req);
        assert_eq!(entry.metrics["SdkS3GetObjectAttempts"].as_u64(), 2);
        assert_eq!(entry.metrics["SdkS3GetObjectAttemptTimeouts"].as_u64(), 1);
        assert_eq!(entry.metrics["SdkS3GetObjectApiCallSuccessful"].as_u64(), 1);
        assert_eq!(entry.metrics["SdkS3GetObjectOperationTimeout"].as_u64(), 0);
    }

    #[tokio::test]
    async fn final_attempt_timeout_is_recorded() {
        let req = request();
        let op = timed_op(
            SlowFirstCalls::new(1, LONG),
            TimeoutConfig::builder()
                .operation_attempt_timeout(SHORT)
                .build(),
            1,
            ClientMetrics::new(),
        );
        with_metrics_pool(req.sdk.handle(), op.invoke(()))
            .await
            .unwrap_err();

        let entry = test_metric(req);
        assert_eq!(entry.metrics["SdkS3GetObjectAttempts"].as_u64(), 1);
        assert_eq!(entry.metrics["SdkS3GetObjectAttemptTimeouts"].as_u64(), 1);
        assert_eq!(entry.metrics["SdkS3GetObjectApiCallSuccessful"].as_u64(), 0);
        assert!(!entry.values.contains_key("SdkS3GetObjectHttpStatusCode"));
        assert_eq!(entry.values["SdkS3GetObjectErrorType"], "ConfiguredTimeout");
    }

    #[tokio::test]
    async fn operation_timeout_still_emits() {
        let req = request();
        let op = timed_op(
            NeverClient::new(),
            TimeoutConfig::builder().operation_timeout(SHORT).build(),
            3,
            ClientMetrics::new(),
        );
        with_metrics_pool(req.sdk.handle(), op.invoke(()))
            .await
            .unwrap_err();

        let entry = test_metric(req);
        assert_eq!(entry.metrics["SdkS3GetObjectOperationTimeout"].as_u64(), 1);
        assert_eq!(entry.metrics["SdkS3GetObjectCancelled"].as_u64(), 0);
        assert_eq!(entry.values["SdkS3GetObjectErrorType"], "ConfiguredTimeout");
        assert_eq!(entry.metrics["SdkS3GetObjectApiCallSuccessful"].as_u64(), 0);
        assert_eq!(entry.metrics["SdkS3GetObjectAttempts"].as_u64(), 1);
        assert!(
            entry.metrics["SdkS3GetObjectApiCallDuration"].as_f64() >= SHORT.as_millis() as f64
        );
    }

    #[tokio::test]
    async fn operation_timeout_writes_one_standalone_entry() {
        let sink = test_entry_sink();
        let op = timed_op(
            NeverClient::new(),
            TimeoutConfig::builder().operation_timeout(SHORT).build(),
            3,
            ClientMetrics::builder()
                .standalone()
                .standalone_sink(sink.sink)
                .build(),
        );
        op.invoke(()).await.unwrap_err();

        let entries = sink.inspector.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].metrics["OperationTimeout"].as_u64(), 1);
    }

    #[tokio::test]
    async fn caller_cancellation_still_emits() {
        let sink = test_entry_sink();
        let op = timed_op(
            NeverClient::new(),
            TimeoutConfig::disabled(),
            3,
            ClientMetrics::builder()
                .standalone()
                .standalone_sink(sink.sink)
                .build(),
        );
        // The caller gives up; the SDK has no timeout configured.
        tokio::time::timeout(SHORT, op.invoke(()))
            .await
            .unwrap_err();

        let entries = sink.inspector.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].metrics["Cancelled"].as_u64(), 1);
        assert!(!entries[0].values.contains_key("ErrorType"));
        assert_eq!(entries[0].metrics["OperationTimeout"].as_u64(), 0);
        assert_eq!(entries[0].metrics["ApiCallSuccessful"].as_u64(), 0);
    }

    #[tokio::test]
    async fn completed_calls_are_not_emitted_twice() {
        let sink = test_entry_sink();
        let op = timed_op(
            SlowFirstCalls::new(0, LONG),
            TimeoutConfig::builder().operation_timeout(LONG).build(),
            3,
            ClientMetrics::builder()
                .standalone()
                .standalone_sink(sink.sink)
                .build(),
        );
        op.invoke(()).await.unwrap();
        let entries = sink.inspector.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].metrics["OperationTimeout"].as_u64(), 0);
        assert_eq!(entries[0].metrics["Cancelled"].as_u64(), 0);
    }

    #[tokio::test]
    async fn attempt_timeout_keeps_connection_telemetry() {
        let req = request();
        let op = timed_op(
            StallsAfterConnecting,
            TimeoutConfig::builder()
                .operation_attempt_timeout(SHORT)
                .build(),
            1,
            ClientMetrics::new(),
        );
        with_metrics_pool(req.sdk.handle(), op.invoke(()))
            .await
            .unwrap_err();

        let entry = test_metric(req);
        assert_eq!(entry.metrics["SdkS3GetObjectAttemptTimeouts"].as_u64(), 1);
        assert_eq!(entry.metrics["SdkS3GetObjectConnectionReused"].as_u64(), 1);
    }

    #[tokio::test]
    async fn operation_timeout_keeps_connection_telemetry() {
        let req = request();
        let op = timed_op(
            StallsAfterConnecting,
            TimeoutConfig::builder().operation_timeout(SHORT).build(),
            1,
            ClientMetrics::new(),
        );
        with_metrics_pool(req.sdk.handle(), op.invoke(()))
            .await
            .unwrap_err();

        let entry = test_metric(req);
        assert_eq!(entry.metrics["SdkS3GetObjectOperationTimeout"].as_u64(), 1);
        assert_eq!(entry.metrics["SdkS3GetObjectConnectionReused"].as_u64(), 1);
    }

    /// The attempt timeout fires, then the operation timeout fires during the
    /// backoff before the next attempt, so no later hook observes the timed-out
    /// attempt.
    #[tokio::test]
    async fn attempt_timeout_followed_by_operation_timeout_in_backoff() {
        let req = request();
        let op = operation(
            "S3",
            "GetObject",
            &[200],
            vec![
                with(ClientMetrics::new()),
                Box::new(|b| {
                    b.http_client(NeverClient::new())
                        .sleep_impl(TokioSleep::new())
                        .timeout_config(
                            TimeoutConfig::builder()
                                .operation_attempt_timeout(SHORT)
                                .operation_timeout(SHORT * 3)
                                .build(),
                        )
                        .retry_classifier(TransientErrorClassifier::<Infallible>::new())
                        // No jitter: the first backoff is exactly 1s, well past
                        // the operation timeout.
                        .standard_retry(
                            &RetryConfig::standard()
                                .with_max_attempts(3)
                                .with_initial_backoff(Duration::from_secs(1))
                                .with_max_backoff(Duration::from_secs(1))
                                .with_use_static_exponential_base(true),
                        )
                }),
            ],
        );
        with_metrics_pool(req.sdk.handle(), op.invoke(()))
            .await
            .unwrap_err();

        let entry = test_metric(req);
        assert_eq!(entry.metrics["SdkS3GetObjectAttempts"].as_u64(), 1);
        assert_eq!(entry.metrics["SdkS3GetObjectAttemptTimeouts"].as_u64(), 1);
        assert_eq!(entry.metrics["SdkS3GetObjectOperationTimeout"].as_u64(), 1);
        assert_eq!(entry.values["SdkS3GetObjectErrorType"], "ConfiguredTimeout");
    }

    /// The operation timeout cuts off an attempt before its own timeout, so the
    /// attempt is not counted as an attempt timeout.
    #[tokio::test]
    async fn operation_timeout_mid_attempt_is_not_an_attempt_timeout() {
        let req = request();
        let op = timed_op(
            NeverClient::new(),
            TimeoutConfig::builder()
                .operation_attempt_timeout(LONG)
                .operation_timeout(SHORT)
                .build(),
            3,
            ClientMetrics::new(),
        );
        with_metrics_pool(req.sdk.handle(), op.invoke(()))
            .await
            .unwrap_err();

        let entry = test_metric(req);
        assert_eq!(entry.metrics["SdkS3GetObjectAttempts"].as_u64(), 1);
        assert_eq!(entry.metrics["SdkS3GetObjectAttemptTimeouts"].as_u64(), 0);
        assert_eq!(entry.metrics["SdkS3GetObjectOperationTimeout"].as_u64(), 1);
    }
}
