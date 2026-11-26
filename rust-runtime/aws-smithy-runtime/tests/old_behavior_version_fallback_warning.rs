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
//! use while their own client serves the traffic. So the warning lives in the client and fires
//! when it is asked for a connector, which is the first point at which it is known to be serving.
//!
//! That is also what these tests drive, rather than a request: handing out a connector is the
//! whole trigger, so nothing here opens a socket. Integration tests so the library is compiled as
//! a user builds it, and so the process-global `Once` that dedupes the warning isn't shared with
//! unrelated unit tests.

// Only a build that lacks a usable legacy client reaches the fallback, so `tls-rustls` must be
// off. `tools/ci-scripts/check-rust-runtimes` runs this combination explicitly.
#![cfg(all(
    feature = "client",
    feature = "test-util",
    feature = "default-https-client",
    not(feature = "tls-rustls")
))]

use aws_smithy_async::rt::sleep::TokioSleep;
use aws_smithy_async::time::SystemTimeSource;
use aws_smithy_runtime::client::defaults::default_http_client_plugin_v2;
use aws_smithy_runtime::test_util::capture_test_logs::capture_test_logs;
use aws_smithy_runtime_api::client::behavior_version::BehaviorVersion;
use aws_smithy_runtime_api::client::http::{HttpClient, HttpConnectorSettings, SharedHttpClient};
use aws_smithy_runtime_api::client::runtime_components::{
    RuntimeComponents, RuntimeComponentsBuilder,
};
use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;

const FALLBACK_WARNING: &str = "the default hyper 1.x HTTPS client is being used instead";

/// A behavior version that asks for the legacy hyper 0.14.x client.
#[allow(deprecated)]
fn old_behavior_version() -> BehaviorVersion {
    BehaviorVersion::v2024_03_28()
}

/// The HTTP client the default plugin installs for `behavior_version`.
fn default_client(behavior_version: BehaviorVersion) -> SharedHttpClient {
    default_http_client_plugin_v2(behavior_version)
        .expect("a default client is available in this build")
        .runtime_components(&RuntimeComponentsBuilder::new("test"))
        .http_client()
        .expect("the plugin installs an HTTP client")
}

/// Components a connector can actually be built against: `for_tests` installs fakes that panic
/// when read, and handing out a connector reads the time source and sleep impl.
fn connector_components() -> RuntimeComponents {
    RuntimeComponentsBuilder::for_tests()
        .with_time_source(Some(SystemTimeSource::new()))
        .with_sleep_impl(Some(TokioSleep::new()))
        .build()
        .expect("test components are complete")
}

#[tokio::test]
async fn fallback_warns_once_and_only_when_asked_for_a_connector() {
    let (_guard, logs) = capture_test_logs();
    let warnings = || logs.contents().matches(FALLBACK_WARNING).count();
    let settings = HttpConnectorSettings::builder().build();
    let components = connector_components();

    // Building the plugin happens for every client and every operation, and a caller's own
    // `http_client` replaces it before any request is sent. So building it must not warn.
    let fallbacks: Vec<_> = (0..3)
        .map(|_| default_client(old_behavior_version()))
        .collect();
    assert_eq!(
        0,
        warnings(),
        "building the default plugin must not warn.\n{}",
        logs.contents()
    );

    // A behavior version that asks for the hyper 1.x client: nothing was substituted.
    let _ = default_client(BehaviorVersion::latest()).http_connector(&settings, &components);
    assert_eq!(
        0,
        warnings(),
        "the latest behavior version must not warn about the legacy stack.\n{}",
        logs.contents()
    );

    // The substituted client handing out connectors: the one case that warns, once per process.
    for fallback in &fallbacks {
        let _ = fallback.http_connector(&settings, &components);
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
    let fallback = default_client(old_behavior_version()).connector_metadata();
    let latest = default_client(BehaviorVersion::latest()).connector_metadata();
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
