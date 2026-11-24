/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Pins when the "legacy hyper 0.14.x client isn't in this build" warning fires.
//!
//! A `BehaviorVersion` older than `v2026_01_12()` asks for the legacy hyper 0.14.x client; in a
//! build without it, `default_http_client_plugin_v2` substitutes the hyper 1.x client and warns.
//! Warning when that plugin is built would be wrong twice over: `default_plugins` runs on every
//! `Client::from_conf` and every `Operation::build()`, and it runs at `Order::Defaults`, ahead of
//! the caller's own config. A caller who supplies an `http_client` would be told the default is in
//! use while their own client serves the traffic.
//!
//! The tests go through `Operation` because that second problem is a consequence of plugin
//! ordering, which a test calling the plugin directly cannot see. They are integration tests so
//! the library is compiled as a user builds it, and so the process-global `Once` that dedupes the
//! warning isn't shared with unrelated tests.

// Only a build that lacks a usable legacy client reaches the fallback, so `tls-rustls` must be
// off. `tools/ci-scripts/check-rust-runtimes` runs this combination explicitly.
#![cfg(all(
    feature = "client",
    feature = "test-util",
    feature = "default-https-client",
    not(feature = "tls-rustls")
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

const FALLBACK_WARNING: &str = "the default hyper 1.x HTTPS client is being used instead";

/// A behavior version that asks for the legacy hyper 0.14.x client.
#[allow(deprecated)]
fn old_behavior_version() -> BehaviorVersion {
    BehaviorVersion::v2024_03_28()
}

type TestOperation = Operation<String, String, Infallible>;
type TestOperationBuilder = OperationBuilder<String, String, Infallible>;

fn operation_builder(behavior_version: BehaviorVersion) -> TestOperationBuilder {
    Operation::builder()
        .service_name("test")
        .operation_name("test")
        .behavior_version(behavior_version)
        // Nothing connects: the default-client cases fail at the transport, after the connector
        // was handed out and the warning with it.
        .endpoint_url("http://localhost:1234")
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
async fn fallback_warning_is_emitted_once_and_only_when_the_fallback_is_used() {
    let (_guard, logs) = capture_test_logs();
    let warnings = || logs.contents().matches(FALLBACK_WARNING).count();

    // A caller's own client: the fallback is never asked for a connector.
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
        "the caller's own HTTP client served every request, so nothing may claim the default \
         hyper 1.x client is being used.\n{}",
        logs.contents()
    );

    // The default client, but a version that asks for it: nothing was substituted.
    let _ = operation_on_default_client(BehaviorVersion::latest())
        .invoke("hello".to_string())
        .await;
    assert_eq!(
        0,
        warnings(),
        "the latest behavior version must not warn about the legacy stack.\n{}",
        logs.contents()
    );

    // The fallback actually serving requests: the one case that warns.
    for _ in 0..3 {
        let _ = operation_on_default_client(old_behavior_version())
            .invoke("hello".to_string())
            .await;
    }
    assert_eq!(
        1,
        warnings(),
        "using the fallback must warn exactly once per process.\n{}",
        logs.contents()
    );
}

/// The wrapper must be transparent apart from the warning.
///
/// `connector_metadata` is the one that fails quietly: `aws-runtime`'s user-agent interceptor
/// reads it off `runtime_components.http_client()`, so dropping it costs a user-agent field
/// rather than breaking a build.
#[test]
fn wrapping_the_fallback_does_not_hide_the_real_client() {
    let metadata_for = |behavior_version| {
        default_http_client_plugin_v2(behavior_version)
            .expect("a default client is available in this build")
            .runtime_components(&RuntimeComponentsBuilder::new("test"))
            .http_client()
            .expect("the plugin installs an HTTP client")
            .connector_metadata()
    };

    let fallback = metadata_for(old_behavior_version());
    let latest = metadata_for(BehaviorVersion::latest());
    assert!(
        latest.is_some(),
        "precondition: the hyper 1.x client reports connector metadata"
    );
    assert_eq!(
        latest.map(|m| m.to_string()),
        fallback.map(|m| m.to_string()),
        "the fallback is the same client, so it must report the same connector metadata"
    );
}
