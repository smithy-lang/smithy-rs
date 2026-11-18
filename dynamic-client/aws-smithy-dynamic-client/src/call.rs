/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Calls to the operations of a dynamic client.

use crate::config;
use crate::error::{DynamicOperationError, OperationNotFound};
use crate::DynamicClient;
use aws_smithy_lang::ShapeId;
use aws_smithy_runtime::client::orchestrator::invoke;
use aws_smithy_runtime_api::client::interceptors::context::Input;
use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
use aws_smithy_runtime_api::client::result::SdkError;
use aws_smithy_types::Document;
use plugin::{operation_plugin, per_call_plugins};
use std::fmt;
use tracing::Instrument;

mod input;
mod output;
mod plugin;

pub use input::DynamicInput;
pub use output::DynamicOutput;

/// The result of one call.
pub type CallResult = Result<DynamicOutput, SdkError<DynamicOperationError, HttpResponse>>;

/// A call to one operation. Create it with [`DynamicClient::call`], and send it with
/// [`send`](CallBuilder::send).
///
/// A call to an operation the service does not have fails when it is sent, with an
/// [`OperationNotFound`] as the source of an `SdkError::ConstructionFailure`.
///
/// The call owns a clone of the client, so the call can move to another task.
#[must_use = "a call does nothing until it is sent"]
pub struct CallBuilder {
    client: DynamicClient,
    operation: Result<ShapeId, OperationNotFound>,
    input: DynamicInput,
    config_override: Option<config::Builder>,
}

impl CallBuilder {
    pub(crate) fn new(
        client: DynamicClient,
        operation: Result<ShapeId, OperationNotFound>,
    ) -> Self {
        Self {
            client,
            operation,
            input: DynamicInput::new(Document::Object(Default::default())),
            config_override: None,
        }
    }

    /// Sets the input. A [`Document`] object keyed by the Smithy member names of the input
    /// converts into a [`DynamicInput`].
    ///
    /// Without an input, the call sends an empty input.
    pub fn input(mut self, input: impl Into<DynamicInput>) -> Self {
        self.input = input.into();
        self
    }

    /// Layers a config over the client's config, for this call only.
    ///
    /// Each field the override sets wins. A second override is layered over the first in the
    /// same way. A behavior version in the override is ignored, because it applies to a whole
    /// client.
    ///
    /// Takes the same types as [`config`](crate::client::DynamicClientBuilder::config).
    pub fn config_override(mut self, config: impl Into<config::Builder>) -> Self {
        let later = config.into();
        self.config_override = Some(match self.config_override.take() {
            Some(earlier) => earlier.merge(later),
            None => later,
        });
        self
    }

    /// Sends the call.
    pub async fn send(self) -> CallResult {
        let operation = self.operation.map_err(SdkError::construction_failure)?;
        let client = self.client;
        let mut runtime_plugins = client
            .runtime_plugins
            .clone()
            .with_operation_plugin(operation_plugin(&client, &operation));
        if let Some(config) = self.config_override {
            for plugin in per_call_plugins(&client, config) {
                runtime_plugins = runtime_plugins.with_operation_plugin(plugin);
            }
        }
        let service = client.sdk_id.as_ref();
        let name = operation.name();
        let output = invoke(service, name, Input::erase(self.input), &runtime_plugins)
            // Generated clients put the same span around each operation.
            .instrument(tracing::debug_span!(
                "operation",
                "rpc.service" = service,
                "rpc.method" = name,
                "sdk_invocation_id" = fastrand::u32(1_000_000..10_000_000),
            ))
            .await
            .map_err(|error| {
                error.map_service_error(|error| {
                    error
                        .downcast::<DynamicOperationError>()
                        .expect("the call's deserializer returns a `DynamicOperationError`")
                })
            })?;
        Ok(output
            .downcast::<DynamicOutput>()
            .expect("the call's deserializer returns a `DynamicOutput`"))
    }
}

// The debug output has no input values, because a value can be `@sensitive`.
impl fmt::Debug for CallBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CallBuilder")
            .field("operation", &self.operation)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{Applies, BehaviorVersion, Config, ConfigKey, RetryConfig};
    use crate::error::OperationNotFound;
    use crate::DynamicClient;
    use aws_smithy_async::test_util::instant_time_and_sleep;
    use aws_smithy_http_client::test_util::{
        capture_request, infallible_client_fn, CaptureRequestReceiver,
    };
    use aws_smithy_runtime::client::orchestrator::endpoints::StaticUriEndpointResolver;
    use aws_smithy_runtime_api::box_error::BoxError;
    use aws_smithy_runtime_api::client::interceptors::{
        context::BeforeSerializationInterceptorContextRef, Intercept,
    };
    use aws_smithy_runtime_api::client::result::SdkError;
    use aws_smithy_runtime_api::client::runtime_components::{
        RuntimeComponents, RuntimeComponentsBuilder,
    };
    use aws_smithy_runtime_api::client::runtime_plugin::StaticRuntimePlugin;
    use aws_smithy_types::body::SdkBody;
    use aws_smithy_types::config_bag::{ConfigBag, Storable, StoreReplace};
    use aws_smithy_types::error::metadata::ProvideErrorMetadata;
    use aws_smithy_types::Document;
    use std::error::Error;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::UNIX_EPOCH;

    // Returns a client for the Sprockets test model. The HTTP client captures the request and
    // returns one response.
    fn client(status: u16, body: &'static str) -> (DynamicClient, CaptureRequestReceiver) {
        let response = http_1x::Response::builder()
            .status(status)
            .body(SdkBody::from(body))
            .unwrap();
        let (http_client, request) = capture_request(Some(response));
        let client = client_with(Config::builder().http_client(http_client));
        (client, request)
    }

    // Returns a client for the Sprockets test model, with `config` layered over the test
    // config. The test config has an HTTP client that returns `{"id":"1"}` to every request.
    fn client_with(config: crate::config::Builder) -> DynamicClient {
        let (_, sleep) = instant_time_and_sleep(UNIX_EPOCH);
        let http_client = infallible_client_fn(|_| {
            http_1x::Response::builder()
                .status(200)
                .body(SdkBody::from(r#"{"id":"1"}"#))
                .unwrap()
        });
        // TODO(dynamic-client): Remove this plugin once the endpoint comes from the model.
        let endpoint = StaticRuntimePlugin::new().with_runtime_components(
            RuntimeComponentsBuilder::new("TestEndpoint").with_endpoint_resolver(Some(
                StaticUriEndpointResolver::uri("https://example.com"),
            )),
        );
        DynamicClient::builder()
            .model_file(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/models/sprockets.json"),
            )
            .config(
                Config::builder()
                    .behavior_version(BehaviorVersion::latest())
                    .http_client(http_client)
                    .sleep_impl(sleep)
                    .runtime_plugin(endpoint),
            )
            .config(config)
            .build()
            .unwrap()
    }

    // `Record` is an interceptor that records one value from the config bag of each call. The
    // interceptor reads the value before serialization, because a client interceptor runs
    // `read_before_execution` before the operation plugins are applied.
    #[derive(Debug)]
    struct Record {
        seen: Arc<Mutex<Vec<String>>>,
        read: fn(&ConfigBag) -> String,
    }

    impl Record {
        fn new(read: fn(&ConfigBag) -> String) -> (Self, Arc<Mutex<Vec<String>>>) {
            let seen = Arc::new(Mutex::new(Vec::new()));
            let record = Self {
                seen: seen.clone(),
                read,
            };
            (record, seen)
        }
    }

    impl Intercept for Record {
        fn name(&self) -> &'static str {
            "Record"
        }

        fn read_before_serialization(
            &self,
            _context: &BeforeSerializationInterceptorContextRef<'_>,
            _components: &RuntimeComponents,
            cfg: &mut ConfigBag,
        ) -> Result<(), BoxError> {
            self.seen.lock().unwrap().push((self.read)(cfg));
            Ok(())
        }
    }

    fn max_attempts(cfg: &ConfigBag) -> String {
        let attempts = cfg
            .load::<RetryConfig>()
            .map(|config| config.max_attempts());
        format!("{attempts:?}")
    }

    fn recorded(seen: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
        seen.lock().unwrap().clone()
    }

    fn sprocket(id: &str) -> Document {
        Document::Object([("id".to_owned(), Document::String(id.to_owned()))].into())
    }

    #[tokio::test]
    async fn a_call_with_no_input_returns_the_output_members() {
        let (client, request) = client(200, r#"{"id":"1"}"#);
        let output = client.call("CreateSprocket").send().await.unwrap();
        assert_eq!(&sprocket("1"), output.members());
        let request = request.expect_request();
        assert_eq!(
            Some("Sprockets.CreateSprocket"),
            request.headers().get("x-amz-target")
        );
        assert_eq!(Some(&b"{}"[..]), request.body().bytes());
    }

    #[tokio::test]
    async fn a_call_sends_the_input_members() {
        let (client, request) = client(200, r#"{"id":"1"}"#);
        let output = client
            .call("GetSprocket")
            .input(sprocket("1"))
            .send()
            .await
            .unwrap();
        assert_eq!(sprocket("1"), output.into_members());
        let request = request.expect_request();
        assert_eq!(Some(&br#"{"id":"1"}"#[..]), request.body().bytes());
    }

    #[tokio::test]
    async fn a_call_to_an_unknown_operation_fails_and_sends_nothing() {
        let (client, request) = client(200, "{}");
        let err = client.call("DeleteSprocket").send().await.unwrap_err();
        assert!(matches!(err, SdkError::ConstructionFailure(_)), "{err:?}");
        let source = err.source().expect("the error has a source");
        let not_found = source
            .downcast_ref::<OperationNotFound>()
            .expect("the source is `OperationNotFound`");
        assert_eq!("DeleteSprocket", not_found.operation());
        assert_eq!("smithy.example#Sprockets", not_found.service().as_str());
        request.expect_no_request();
    }

    #[tokio::test]
    async fn an_error_response_keeps_its_code_and_message() {
        let (client, _request) = client(
            400,
            r#"{"__type":"smithy.example#InvalidSprocketId","message":"bad id"}"#,
        );
        let err = client
            .call("GetSprocket")
            .input(sprocket("?"))
            .send()
            .await
            .unwrap_err();
        let err = err
            .as_service_error()
            .expect("the response is a service error");
        assert_eq!(Some("InvalidSprocketId"), err.code());
        assert_eq!(Some("bad id"), err.message());
    }

    #[tokio::test]
    async fn a_per_call_interceptor_runs_only_on_that_call() {
        let client = client_with(Config::builder());
        let (record, seen) = Record::new(|_| "ran".to_owned());
        let call = || client.call("CreateSprocket");
        call()
            .config_override(Config::builder().interceptor(record))
            .send()
            .await
            .unwrap();
        call().send().await.unwrap();
        assert_eq!(vec!["ran"], recorded(&seen));
    }

    #[tokio::test]
    async fn a_per_call_retry_config_applies_only_to_that_call() {
        let (record, seen) = Record::new(max_attempts);
        let client = client_with(
            Config::builder()
                .retry_config(RetryConfig::disabled())
                .interceptor(record),
        );
        let call = || client.call("CreateSprocket");
        call()
            .config_override(
                Config::builder().retry_config(RetryConfig::standard().with_max_attempts(5)),
            )
            .send()
            .await
            .unwrap();
        call().send().await.unwrap();
        assert_eq!(vec!["Some(5)", "Some(1)"], recorded(&seen));
    }

    #[tokio::test]
    async fn a_later_override_wins_and_both_add_interceptors() {
        let (first, first_seen) = Record::new(max_attempts);
        let (second, second_seen) = Record::new(max_attempts);
        let client = client_with(Config::builder());
        client
            .call("CreateSprocket")
            .config_override(
                Config::builder()
                    .retry_config(RetryConfig::standard().with_max_attempts(2))
                    .interceptor(first),
            )
            .config_override(
                Config::builder()
                    .retry_config(RetryConfig::standard().with_max_attempts(7))
                    .interceptor(second),
            )
            .send()
            .await
            .unwrap();
        assert_eq!(vec!["Some(7)"], recorded(&first_seen));
        assert_eq!(vec!["Some(7)"], recorded(&second_seen));
    }

    #[tokio::test]
    async fn per_call_timeouts_keep_the_timeouts_they_leave_unset() {
        use aws_smithy_types::timeout::{MergeTimeoutConfig, TimeoutConfig};
        use std::time::Duration;
        let (record, seen) = Record::new(|cfg| {
            let timeouts = cfg.load::<MergeTimeoutConfig>();
            format!(
                "{:?} {:?} {:?}",
                timeouts.connect_timeout(),
                timeouts.read_timeout(),
                timeouts.operation_timeout()
            )
        });
        let connect = TimeoutConfig::builder()
            .connect_timeout(Duration::from_secs(10))
            .build();
        let operation = TimeoutConfig::builder()
            .operation_timeout(Duration::from_secs(30))
            .build();
        let read = TimeoutConfig::builder()
            .read_timeout(Duration::from_secs(5))
            .build();
        let client = client_with(
            Config::builder()
                .timeout_config(connect)
                .interceptor(record),
        );
        client
            .call("CreateSprocket")
            .config_override(Config::builder().timeout_config(operation))
            .config_override(Config::builder().timeout_config(read))
            .send()
            .await
            .unwrap();
        assert_eq!(vec!["Some(10s) Some(5s) Some(30s)"], recorded(&seen));
    }

    #[derive(Clone, Debug)]
    struct Tag(&'static str);

    impl Storable for Tag {
        type Storer = StoreReplace<Self>;
    }

    impl ConfigKey for Tag {
        const APPLIES: &'static [Applies] = &[];
    }

    #[tokio::test]
    async fn a_per_call_put_applies_only_to_that_call() {
        let (record, seen) = Record::new(|cfg| format!("{:?}", cfg.load::<Tag>().map(|tag| tag.0)));
        let client = client_with(Config::builder().put(Tag("client")).interceptor(record));
        let call = || client.call("CreateSprocket");
        call()
            .config_override(Config::builder().put(Tag("call")))
            .send()
            .await
            .unwrap();
        call().send().await.unwrap();
        assert_eq!(
            vec![r#"Some("call")"#, r#"Some("client")"#],
            recorded(&seen)
        );
    }

    #[cfg(feature = "aws")]
    #[tokio::test]
    async fn a_per_call_region_applies_only_to_that_call() {
        use crate::config::Region;
        let (record, seen) =
            Record::new(|cfg| format!("{:?}", cfg.load::<Region>().map(|r| r.as_ref().to_owned())));
        let client = client_with(
            Config::builder()
                .region(Region::new("us-west-2"))
                .interceptor(record),
        );
        let call = || client.call("CreateSprocket");
        call()
            .config_override(Config::builder().region(Region::new("eu-west-1")))
            .send()
            .await
            .unwrap();
        call().send().await.unwrap();
        assert_eq!(
            vec![r#"Some("eu-west-1")"#, r#"Some("us-west-2")"#],
            recorded(&seen)
        );
    }
}
