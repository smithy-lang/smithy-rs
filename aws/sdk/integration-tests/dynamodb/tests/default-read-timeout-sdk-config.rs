/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Resolution of the default-read-timeout opt-in for a client built from `SdkConfig`.
//!
//! On that path the opt-in is resolved through `aws-config`, which reads the environment via an
//! `Env` shim rather than calling `std::env::var`. Two things follow, and both are asserted here:
//! test utilities can isolate a client from the host environment, and the answer is fixed when the
//! client is built rather than re-read afterwards.
//!
//! The companion file `default-read-timeout.rs` covers the other construction path, a bare
//! `Config::builder()`, where no `SdkConfig` and therefore no `Env` exists.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use aws_config::BehaviorVersion;
use aws_credential_types::provider::SharedCredentialsProvider;
use aws_runtime::env_config::EnvConfigValue;
use aws_sdk_dynamodb::config::{Credentials, Region};
use aws_smithy_http_client::test_util::NeverClient;
use aws_smithy_runtime_api::client::http::{http_client_fn, HttpClient, SharedHttpClient};
use aws_smithy_types::retry::RetryConfig;
use aws_types::os_shim_internal::Env;
use aws_types::service_config::{LoadServiceConfig, ServiceConfigKey};
use aws_types::SdkConfig;

const OPT_IN: &str = "AWS_ENABLE_DEFAULT_SOCKET_TIMEOUT_2026";
const FIVE_MINUTES: Duration = Duration::from_secs(300);

/// A `LoadServiceConfig` over a faked environment, the way `aws-config` does it internally.
///
/// Using this rather than `std::env::set_var` keeps these cases off the process environment
/// entirely, so they neither race each other nor depend on the host.
#[derive(Debug)]
struct FakeEnv {
    env: Env,
}

impl FakeEnv {
    fn new(vars: &[(&str, &str)]) -> Self {
        Self {
            env: Env::from_slice(vars),
        }
    }
}

impl LoadServiceConfig for FakeEnv {
    fn load_config(&self, key: ServiceConfigKey<'_>) -> Option<String> {
        let (value, _source) = EnvConfigValue::new()
            .env(key.env())
            .profile(key.profile())
            .service_id(key.service_id())
            .load(&self.env, None)?;
        Some(value.to_string())
    }
}

/// An HTTP client that never responds, recording the `read_timeout` it is handed.
fn capturing_client() -> (SharedHttpClient, Arc<Mutex<Option<Option<Duration>>>>) {
    let captured: Arc<Mutex<Option<Option<Duration>>>> = Default::default();
    let sink = captured.clone();
    let inner = NeverClient::new();
    let http_client = http_client_fn(move |settings, components| {
        *sink.lock().unwrap() = Some(settings.read_timeout());
        inner.http_connector(settings, components)
    });
    (http_client, captured)
}

/// Sends one request and returns the `read_timeout` the HTTP client was handed.
async fn read_timeout_of(
    client: aws_sdk_dynamodb::Client,
    captured: Arc<Mutex<Option<Option<Duration>>>>,
) -> Option<Duration> {
    // `NeverClient` never responds, so bound the call; the settings are captured before the
    // connector is ever polled.
    let _ = tokio::time::timeout(Duration::from_millis(300), client.list_tables().send()).await;
    captured
        .lock()
        .unwrap()
        .take()
        .expect("the HTTP client must have been asked for a connector")
}

/// Builds a client from `SdkConfig` whose service config is the given faked environment.
async fn read_timeout_with_fake_env(vars: &[(&str, &str)]) -> Option<Duration> {
    let (http_client, captured) = capturing_client();
    let sdk_config = SdkConfig::builder()
        .region(Region::new("us-east-2"))
        .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
        .retry_config(RetryConfig::disabled())
        .http_client(http_client)
        .behavior_version(BehaviorVersion::latest())
        .service_config(FakeEnv::new(vars))
        .build();
    read_timeout_of(aws_sdk_dynamodb::Client::new(&sdk_config), captured).await
}

#[tokio::test]
async fn opt_in_through_service_config_enables_the_default() {
    assert_eq!(
        read_timeout_with_fake_env(&[(OPT_IN, "true")]).await,
        Some(FIVE_MINUTES),
        "the opt-in must be honored when it arrives through aws-config's service config"
    );
}

#[tokio::test]
async fn absent_from_service_config_leaves_the_default_off() {
    assert_eq!(
        read_timeout_with_fake_env(&[]).await,
        None,
        "no opt-in means no default read timeout"
    );
}

#[tokio::test]
async fn the_opt_in_is_case_insensitive_through_service_config() {
    for value in ["True", "TRUE", "tRuE"] {
        assert_eq!(
            read_timeout_with_fake_env(&[(OPT_IN, value)]).await,
            Some(FIVE_MINUTES),
            "{value:?} must enable the default"
        );
    }
    for value in ["", "false", "1", "yes"] {
        assert_eq!(
            read_timeout_with_fake_env(&[(OPT_IN, value)]).await,
            None,
            "{value:?} must not enable the default"
        );
    }
}

/// The generic resolver also accepts a service-scoped form of the variable.
///
/// This is not a designed feature of the opt-in; it falls out of `EnvConfigValue`'s resolution
/// order, which checks `{VAR}_{SERVICE_ID}` before the bare `{VAR}`. Asserted so the behavior is
/// recorded rather than discovered.
#[tokio::test]
async fn a_service_scoped_form_of_the_variable_also_works() {
    assert_eq!(
        read_timeout_with_fake_env(&[("AWS_ENABLE_DEFAULT_SOCKET_TIMEOUT_2026_DYNAMODB", "true")])
            .await,
        Some(FIVE_MINUTES),
    );
}

/// Loads shared config the way an application does, so the host environment is actually consulted.
async fn read_timeout_via_loader(isolate: bool) -> Option<Duration> {
    let (http_client, captured) = capturing_client();
    let mut loader = aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new("us-east-2"))
        .credentials_provider(Credentials::for_tests())
        .retry_config(RetryConfig::disabled())
        .http_client(http_client);
    if isolate {
        loader = loader.empty_test_environment();
    }
    let sdk_config = loader.load().await;
    read_timeout_of(aws_sdk_dynamodb::Client::new(&sdk_config), captured).await
}

/// `empty_test_environment()` must isolate a client from a host-set opt-in.
///
/// This is the case that raw `std::env::var` could not satisfy, and it is why the opt-in is
/// resolved through `aws-config` on this path. Both halves run inside one test because the host
/// environment is process state, so two tests would overwrite each other.
#[tokio::test]
async fn empty_test_environment_isolates_the_client_from_the_opt_in() {
    std::env::set_var(OPT_IN, "true");
    let from_host = read_timeout_via_loader(false).await;
    let isolated = read_timeout_via_loader(true).await;
    std::env::remove_var(OPT_IN);

    assert_eq!(
        from_host,
        Some(FIVE_MINUTES),
        "the opt-in must reach a client built from loaded shared config"
    );
    assert_eq!(
        isolated, None,
        "a host-only opt-in must not reach a client built with empty_test_environment(), which \
         already guarantees this for settings such as AWS_RETRY_MODE and AWS_ENDPOINT_URL"
    );
}
