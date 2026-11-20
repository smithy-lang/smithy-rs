/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// The SigV4 and SigV4a auth schemes, set up from the service's `@aws.auth#sigv4` and
// `@aws.auth#sigv4a` traits as generated clients set them up.

use aws_runtime::auth::{PayloadSigningOverride, SigV4OperationSigningConfig, SigningOptions};
use aws_smithy_lang::shape::{OperationView, ServiceView};
use aws_smithy_lang::ShapeId;
use aws_smithy_runtime_api::client::auth::AuthSchemeId;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
use aws_smithy_types::config_bag::Layer;
use aws_types::SigningName;

const SIGV4: ShapeId = ShapeId::from_static("aws.auth#sigv4");
const SIGV4A: ShapeId = ShapeId::from_static("aws.auth#sigv4a");
const UNSIGNED_PAYLOAD: ShapeId = ShapeId::from_static("aws.auth#unsignedPayload");

// Registers the SigV4 and SigV4a auth schemes for a service with their traits. The signing name
// is stored from the `name` of the SigV4 trait, or else of the SigV4a trait.
pub(crate) fn store_service_auth(
    service: &ServiceView<'_>,
    layer: &mut Layer,
    components: &mut RuntimeComponentsBuilder,
) {
    let signing_name = [SIGV4, SIGV4A]
        .iter()
        .filter_map(|id| service.get_trait(id))
        .find_map(|value| value.as_object()?.get("name")?.as_str());
    if let Some(signing_name) = signing_name {
        layer.store_put(SigningName::from(signing_name.to_owned()));
    }
    if service.has_trait(&SIGV4) {
        components.push_auth_scheme(aws_runtime::auth::sigv4::SigV4AuthScheme::new());
    }
    if service.has_trait(&SIGV4A) {
        components.push_auth_scheme(aws_runtime::auth::sigv4a::SigV4aAuthScheme::new());
    }
}

// Stores the signing config of an operation that SigV4 or SigV4a may sign. An operation with
// `@unsignedPayload` is signed with no payload hash.
pub(crate) fn store_operation_auth(
    operation: &OperationView<'_>,
    auth_schemes: &[AuthSchemeId],
    layer: &mut Layer,
) {
    let signs = auth_schemes
        .iter()
        .any(|id| *id == aws_runtime::auth::sigv4::SCHEME_ID || id.inner() == SIGV4A.name());
    if !signs {
        return;
    }
    let unsigned_payload = operation.has_trait(&UNSIGNED_PAYLOAD);
    let mut signing_options = SigningOptions::default();
    signing_options.content_sha256_header = unsigned_payload;
    signing_options.payload_override =
        unsigned_payload.then(|| PayloadSigningOverride::unsigned_payload().to_signable_body());
    layer.store_put(SigV4OperationSigningConfig {
        signing_options,
        ..Default::default()
    });
}

#[cfg(test)]
mod tests {
    use crate::config::{BehaviorVersion, Config, Region};
    use crate::DynamicClient;
    use aws_credential_types::provider::SharedCredentialsProvider;
    use aws_credential_types::Credentials;
    use aws_smithy_async::time::StaticTimeSource;
    use aws_smithy_http_client::test_util::infallible_client_fn;
    use aws_smithy_lang::ShapeId;
    use aws_smithy_runtime::client::orchestrator::endpoints::StaticUriEndpointResolver;
    use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
    use aws_smithy_runtime_api::client::runtime_plugin::StaticRuntimePlugin;
    use aws_smithy_types::body::SdkBody;
    use http_1x::HeaderMap;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, UNIX_EPOCH};

    // The headers of each request a test client sent, in order.
    type Sent = Arc<Mutex<Vec<HeaderMap>>>;

    // Returns a client for `service` of the SigV4 test model, with `config` layered over the test
    // config. The test config signs with test credentials in `us-west-2`, on 2024-01-01.
    fn test_client(service: &str, config: crate::config::Builder) -> (DynamicClient, Sent) {
        let sent = Sent::default();
        let seen = sent.clone();
        let http_client = infallible_client_fn(move |request| {
            seen.lock().unwrap().push(request.headers().clone());
            http_1x::Response::builder()
                .status(200)
                .body(SdkBody::from("{}"))
                .unwrap()
        });
        // TODO(dynamic-client): Remove this plugin once the endpoint comes from the model.
        let endpoint = StaticRuntimePlugin::new().with_runtime_components(
            RuntimeComponentsBuilder::new("TestEndpoint").with_endpoint_resolver(Some(
                StaticUriEndpointResolver::uri("https://example.com"),
            )),
        );
        let client = DynamicClient::builder()
            .model_file(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/models/sigv4.json"))
            .service(ShapeId::new(service).unwrap())
            .config(
                Config::builder()
                    .behavior_version(BehaviorVersion::latest())
                    .http_client(http_client)
                    .time_source(StaticTimeSource::new(
                        UNIX_EPOCH + Duration::from_secs(1_704_067_200),
                    ))
                    .region(Region::new("us-west-2"))
                    .credentials_provider(Credentials::for_tests())
                    .runtime_plugin(endpoint),
            )
            .config(config)
            .build()
            .unwrap();
        (client, sent)
    }

    const SIGNED: &str = "smithy.example#Signed";

    fn header(sent: &Sent, index: usize, name: &str) -> Option<String> {
        sent.lock().unwrap()[index]
            .get(name)
            .map(|value| value.to_str().unwrap().to_owned())
    }

    #[tokio::test]
    async fn sigv4_signs_with_the_signing_name_and_the_region() {
        let (client, sent) = test_client(SIGNED, Config::builder());
        client.call("GetThing").send().await.unwrap();
        let authorization = header(&sent, 0, "authorization").unwrap();
        assert!(authorization.starts_with(
            "AWS4-HMAC-SHA256 Credential=ANOTREAL/20240101/us-west-2/gizmo/aws4_request"
        ));
        assert_eq!(None, header(&sent, 0, "x-amz-content-sha256"));
    }

    #[tokio::test]
    async fn sigv4_signs_an_unsigned_payload_operation_with_no_payload_hash() {
        let (client, sent) = test_client(SIGNED, Config::builder());
        client.call("PutThing").send().await.unwrap();
        assert_eq!(
            Some("UNSIGNED-PAYLOAD".to_owned()),
            header(&sent, 0, "x-amz-content-sha256")
        );
    }

    #[tokio::test]
    async fn a_per_call_region_is_the_signing_region_of_that_call() {
        let (client, sent) = test_client(SIGNED, Config::builder());
        client
            .call("GetThing")
            .config_override(Config::builder().region(Region::new("eu-west-1")))
            .send()
            .await
            .unwrap();
        client.call("GetThing").send().await.unwrap();
        let scope = |index| {
            let authorization = header(&sent, index, "authorization").unwrap();
            authorization.split('/').nth(2).unwrap().to_owned()
        };
        assert_eq!("eu-west-1", scope(0));
        assert_eq!("us-west-2", scope(1));
    }

    #[tokio::test]
    async fn the_last_identity_resolver_or_credentials_provider_for_sigv4_wins() {
        let other = || {
            SharedCredentialsProvider::new(Credentials::new("OTHER", "secret", None, None, "test"))
        };
        // The test config sets the credentials provider first, so the resolver set later wins.
        let (client, sent) = test_client(
            SIGNED,
            Config::builder().identity_resolver(aws_runtime::auth::sigv4::SCHEME_ID, other()),
        );
        // A credentials provider set after the resolver wins.
        let (later_client, later_sent) = test_client(
            SIGNED,
            Config::builder()
                .identity_resolver(aws_runtime::auth::sigv4::SCHEME_ID, other())
                .credentials_provider(Credentials::for_tests()),
        );
        client.call("GetThing").send().await.unwrap();
        later_client.call("GetThing").send().await.unwrap();
        let access_key = |sent: &Sent| {
            let authorization = header(sent, 0, "authorization").unwrap();
            authorization.split(['=', '/']).nth(1).unwrap().to_owned()
        };
        assert_eq!("OTHER", access_key(&sent));
        assert_eq!("ANOTREAL", access_key(&later_sent));
    }

    #[tokio::test]
    async fn sigv4a_signs_for_the_region_by_default() {
        let (client, sent) = test_client("smithy.example#SignedA", Config::builder());
        client.call("GetThing").send().await.unwrap();
        let authorization = header(&sent, 0, "authorization").unwrap();
        assert!(authorization
            .starts_with("AWS4-ECDSA-P256-SHA256 Credential=ANOTREAL/20240101/gizmo/aws4_request"));
        assert_eq!(
            Some("us-west-2".to_owned()),
            header(&sent, 0, "x-amz-region-set")
        );
    }

    #[tokio::test]
    async fn sigv4a_signs_for_the_signing_region_set_of_an_sdk_config() {
        let sdk_config = aws_types::SdkConfig::builder()
            .sigv4a_signing_region_set("*")
            .build();
        let (client, sent) = test_client(
            "smithy.example#SignedA",
            crate::config::Builder::from(&sdk_config),
        );
        client.call("GetThing").send().await.unwrap();
        assert_eq!(Some("*".to_owned()), header(&sent, 0, "x-amz-region-set"));
    }
}
