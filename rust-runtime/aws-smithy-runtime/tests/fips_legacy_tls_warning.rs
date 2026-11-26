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
//! Warning when the default plugin is built would be wrong: `default_plugins` runs on every
//! `Client::from_conf` and every `Operation::build()`, at `Order::Defaults`, ahead of the caller's
//! own config. A caller who supplies an `http_client` would be told their TLS is not FIPS while
//! their own client serves the traffic. So the warning lives in the client and fires when it is
//! asked for a connector, which is the first point at which it is known to be serving.
//!
//! That is also what these tests drive, rather than a request: handing out a connector is the
//! whole trigger, so nothing here opens a socket. Integration tests so the library is compiled as
//! a user builds it, and so the process-global `Once` that dedupes the warning isn't shared with
//! unrelated unit tests.

// The legacy client has to actually yield something, which needs a legacy TLS implementation, and
// there is no FIPS claim to contradict without `aws-lc-fips`. The `--all-features` leg in
// `tools/ci-scripts/check-rust-runtimes` runs this combination.
#![cfg(all(
    feature = "client",
    feature = "test-util",
    feature = "tls-rustls",
    feature = "aws-lc-fips"
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

const NOT_FIPS_WARNING: &str = "is not end-to-end FIPS";

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
async fn fips_warning_is_emitted_once_and_only_when_the_legacy_client_serves() {
    let (_guard, logs) = capture_test_logs();
    let warnings = || logs.contents().matches(NOT_FIPS_WARNING).count();
    let settings = HttpConnectorSettings::builder().build();
    let components = connector_components();

    // Building the plugin happens for every client and every operation, and a caller's own
    // `http_client` replaces it before any request is sent. So building it must not warn.
    let legacy: Vec<_> = (0..3)
        .map(|_| default_client(old_behavior_version()))
        .collect();
    assert_eq!(
        0,
        warnings(),
        "building the default plugin must not warn.\n{}",
        logs.contents()
    );

    // A version that selects the hyper 1.x client: `aws-lc-fips` does reach that TLS, so there is
    // nothing to warn about.
    let _ = default_client(BehaviorVersion::latest()).http_connector(&settings, &components);
    assert_eq!(
        0,
        warnings(),
        "a FIPS build whose TLS is FIPS must not warn that it isn't.\n{}",
        logs.contents()
    );

    // The legacy client handing out connectors: the one case that warns, once per process.
    for client in &legacy {
        let _ = client.http_connector(&settings, &components);
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
///
/// Asserted as present rather than equal to the hyper 1.x client's, because the wrapped client
/// here is the legacy one and the two legitimately report different connectors.
#[test]
fn wrapping_the_legacy_client_does_not_hide_the_real_client() {
    assert!(
        default_client(BehaviorVersion::latest())
            .connector_metadata()
            .is_some(),
        "precondition: the hyper 1.x client reports connector metadata"
    );
    assert!(
        default_client(old_behavior_version())
            .connector_metadata()
            .is_some(),
        "the wrapped legacy client must still report its connector metadata"
    );
}
