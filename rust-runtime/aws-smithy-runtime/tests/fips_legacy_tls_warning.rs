/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Pins when the "a FIPS build resolved to the legacy TLS stack" warning fires.
//!
//! `aws-lc-fips` reaches TLS only through the hyper 1.x client. A `BehaviorVersion` older than
//! `v2026_01_12()` asks for the legacy hyper 0.14.x client instead, whose TLS is `rustls` 0.21 on
//! `ring`, so such a build is not end-to-end FIPS and is told so.
//!
//! The warning comes from the client rather than from where the client is chosen, because
//! `default_http_client_plugin_v2` runs on every `Client::from_conf` and every `Operation::build()`
//! at `Order::Defaults`, ahead of the caller's own components. A caller who supplies an
//! `http_client` would otherwise be told their TLS is not FIPS while their own client serves the
//! traffic. These are integration tests for the same reasons as
//! `old_behavior_version_fallback_warning.rs`: the ordering is only visible through `Operation`,
//! and the process-global `Once` that dedupes the warning must not be shared with unrelated tests.

// The legacy client has to actually yield something, which needs a legacy TLS implementation, and
// there is no FIPS claim to contradict without `aws-lc-fips`. The `--all-features` leg in
// `tools/ci-scripts/check-rust-runtimes` runs this combination.
#![cfg(all(
    feature = "client",
    feature = "test-util",
    feature = "tls-rustls",
    feature = "aws-lc-fips"
))]

use aws_smithy_http_client::test_util::capture_request;
use aws_smithy_runtime::client::defaults::default_http_client_plugin_v2;
use aws_smithy_runtime::client::orchestrator::operation::{Operation, OperationBuilder};
use aws_smithy_runtime::test_util::capture_test_logs::capture_test_logs;
use aws_smithy_runtime_api::client::behavior_version::BehaviorVersion;
use aws_smithy_runtime_api::client::http::HttpClient;
use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
use aws_smithy_types::body::SdkBody;
use aws_smithy_types::timeout::TimeoutConfig;
use std::convert::Infallible;

const NOT_FIPS_WARNING: &str = "is not end-to-end FIPS";

/// A behavior version that asks for the legacy hyper 0.14.x client.
#[allow(deprecated)]
fn old_behavior_version() -> BehaviorVersion {
    BehaviorVersion::v2024_03_28()
}

type TestOperation = Operation<String, String, Infallible>;
type TestOperationBuilder = OperationBuilder<String, String, Infallible>;

/// An endpoint with nothing listening, so a default-client request fails at connect.
///
/// Not a fixed port: one can be in use on a developer machine, and with timeouts disabled a
/// request that does connect waits for a response forever.
fn closed_port_endpoint() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    let port = listener.local_addr().expect("local address").port();
    drop(listener);
    format!("http://127.0.0.1:{port}")
}

fn operation_builder(behavior_version: BehaviorVersion) -> TestOperationBuilder {
    Operation::builder()
        .service_name("test")
        .operation_name("test")
        .behavior_version(behavior_version)
        // Failing at connect is still after the connector was handed out, and the warning with it.
        .endpoint_url(&closed_port_endpoint())
        .no_auth()
        .no_retry()
        .timeout_config(TimeoutConfig::disabled())
        .serializer(|input: String| Ok(HttpRequest::new(SdkBody::from(input.as_bytes()))))
        .deserializer::<_, Infallible>(|_response| Ok("ok".to_string()))
}

/// Built as generated clients and `aws-config`'s credential providers are: `default_plugins`
/// first, the caller's HTTP client over it.
fn operation_with(
    behavior_version: BehaviorVersion,
    http_client: impl HttpClient + 'static,
) -> TestOperation {
    operation_builder(behavior_version)
        .http_client(http_client)
        .build()
}

/// Built with no HTTP client of its own, so it runs on whatever the default plugin installed.
fn operation_on_default_client(behavior_version: BehaviorVersion) -> TestOperation {
    operation_builder(behavior_version).build()
}

fn ok_response() -> http_1x::Response<SdkBody> {
    http_1x::Response::builder()
        .status(200)
        .body(SdkBody::empty())
        .unwrap()
}

#[tokio::test]
async fn fips_warning_is_emitted_once_and_only_when_the_legacy_client_serves() {
    let (_guard, logs) = capture_test_logs();
    let warnings = || logs.contents().matches(NOT_FIPS_WARNING).count();

    // A caller's own client: the legacy client is never asked for a connector, so claiming this
    // build's TLS is not FIPS would be wrong -- their client may well be a FIPS one.
    for _ in 0..3 {
        let (http_client, rx) = capture_request(Some(ok_response()));
        let output = operation_with(old_behavior_version(), http_client)
            .invoke("hello".to_string())
            .await
            .expect("the caller's client returns the canned response");
        assert_eq!("ok", output);
        let request = rx.expect_request();
        assert_eq!(b"hello", request.body().bytes().unwrap());
    }
    assert_eq!(
        0,
        warnings(),
        "the caller's own HTTP client served every request, so nothing may claim this build's TLS \
         is not FIPS.\n{}",
        logs.contents()
    );

    // A version that selects the hyper 1.x client: `aws-lc-fips` does reach that TLS, so there is
    // nothing to warn about.
    let _ = operation_on_default_client(BehaviorVersion::latest())
        .invoke("hello".to_string())
        .await;
    assert_eq!(
        0,
        warnings(),
        "a FIPS build whose TLS is FIPS must not warn that it isn't.\n{}",
        logs.contents()
    );

    // The legacy client actually serving requests: the one case that warns.
    for _ in 0..3 {
        let _ = operation_on_default_client(old_behavior_version())
            .invoke("hello".to_string())
            .await;
    }
    assert_eq!(
        1,
        warnings(),
        "a FIPS build on the legacy TLS stack must warn exactly once per process.\n{}",
        logs.contents()
    );
    assert!(
        logs.contents().contains("v2026_01_12"),
        "the warning must name the behavior version that selects the FIPS-capable client.\n{}",
        logs.contents()
    );
}

/// The wrapper must be transparent apart from the warning.
///
/// `connector_metadata` is the one that fails quietly: `aws-runtime`'s user-agent interceptor
/// reads it off `runtime_components.http_client()`, so dropping it costs a user-agent field
/// rather than breaking a build.
#[test]
fn wrapping_the_legacy_client_does_not_hide_the_real_client() {
    let metadata_for = |behavior_version| {
        default_http_client_plugin_v2(behavior_version)
            .expect("a default client is available in this build")
            .runtime_components(&RuntimeComponentsBuilder::new("test"))
            .http_client()
            .expect("the plugin installs an HTTP client")
            .connector_metadata()
    };

    assert!(
        metadata_for(BehaviorVersion::latest()).is_some(),
        "precondition: the hyper 1.x client reports connector metadata"
    );
    assert!(
        metadata_for(old_behavior_version()).is_some(),
        "the wrapped legacy client must still report its connector metadata"
    );
}
