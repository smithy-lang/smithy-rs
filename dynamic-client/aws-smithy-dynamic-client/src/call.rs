/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Calls to the operations of a dynamic client.

use crate::error::{DynamicOperationError, OperationNotFound};
use crate::DynamicClient;
use aws_smithy_lang::ShapeId;
use aws_smithy_runtime::client::orchestrator::invoke;
use aws_smithy_runtime_api::client::interceptors::context::Input;
use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
use aws_smithy_runtime_api::client::result::SdkError;
use aws_smithy_types::Document;
use plugin::operation_plugin;
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

    /// Sends the call.
    pub async fn send(self) -> CallResult {
        let operation = self.operation.map_err(SdkError::construction_failure)?;
        let client = self.client;
        let runtime_plugins = client
            .runtime_plugins
            .clone()
            .with_operation_plugin(operation_plugin(&client, &operation));
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
    use crate::config::{BehaviorVersion, Config};
    use crate::error::OperationNotFound;
    use crate::DynamicClient;
    use aws_smithy_async::test_util::instant_time_and_sleep;
    use aws_smithy_http_client::test_util::{capture_request, CaptureRequestReceiver};
    use aws_smithy_runtime::client::orchestrator::endpoints::StaticUriEndpointResolver;
    use aws_smithy_runtime_api::client::result::SdkError;
    use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
    use aws_smithy_runtime_api::client::runtime_plugin::StaticRuntimePlugin;
    use aws_smithy_types::body::SdkBody;
    use aws_smithy_types::error::metadata::ProvideErrorMetadata;
    use aws_smithy_types::Document;
    use std::error::Error;
    use std::path::PathBuf;
    use std::time::UNIX_EPOCH;

    // Returns a client for the Sprockets test model. The HTTP client captures the request and
    // returns one response.
    fn client(status: u16, body: &'static str) -> (DynamicClient, CaptureRequestReceiver) {
        let response = http_1x::Response::builder()
            .status(status)
            .body(SdkBody::from(body))
            .unwrap();
        let (http_client, request) = capture_request(Some(response));
        let (_, sleep) = instant_time_and_sleep(UNIX_EPOCH);
        // TODO(dynamic-client): Remove this plugin once the endpoint comes from the model.
        let endpoint = StaticRuntimePlugin::new().with_runtime_components(
            RuntimeComponentsBuilder::new("TestEndpoint").with_endpoint_resolver(Some(
                StaticUriEndpointResolver::uri("https://example.com"),
            )),
        );
        let client = DynamicClient::builder()
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
            .build()
            .unwrap();
        (client, request)
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
}
