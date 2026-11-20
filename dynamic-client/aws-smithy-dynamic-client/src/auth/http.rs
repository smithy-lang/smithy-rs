/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// The HTTP auth schemes, registered from the service's HTTP auth traits as generated clients
// register them.

use aws_smithy_lang::shape::ServiceView;
use aws_smithy_lang::ShapeId;
use aws_smithy_runtime::client::auth::http::{
    ApiKeyAuthScheme, ApiKeyLocation, BasicAuthScheme, BearerAuthScheme,
};
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;

const HTTP_API_KEY_AUTH: ShapeId = ShapeId::from_static("smithy.api#httpApiKeyAuth");
const HTTP_BASIC_AUTH: ShapeId = ShapeId::from_static("smithy.api#httpBasicAuth");
const HTTP_BEARER_AUTH: ShapeId = ShapeId::from_static("smithy.api#httpBearerAuth");

// Registers an auth scheme for each HTTP auth trait of the service. `@httpDigestAuth` gets no
// scheme, because the runtime has no Digest signer.
pub(crate) fn store_service_auth(
    service: &ServiceView<'_>,
    components: &mut RuntimeComponentsBuilder,
) {
    if let Some(api_key) = service.get_trait(&HTTP_API_KEY_AUTH) {
        // TODO(aws-smithy-lang): Decode `@httpApiKeyAuth` with a trait codec, once the crate has
        // one.
        let value = api_key.as_object();
        let field = |name| value.and_then(|value| value.get(name)?.as_str());
        let location = match field("in") {
            Some("query") => ApiKeyLocation::Query,
            _ => ApiKeyLocation::Header,
        };
        components.push_auth_scheme(ApiKeyAuthScheme::new(
            field("scheme").unwrap_or_default(),
            location,
            field("name").unwrap_or_default(),
        ));
    }
    if service.has_trait(&HTTP_BASIC_AUTH) {
        components.push_auth_scheme(BasicAuthScheme::new());
    }
    if service.has_trait(&HTTP_BEARER_AUTH) {
        components.push_auth_scheme(BearerAuthScheme::new());
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{BehaviorVersion, Config};
    use crate::DynamicClient;
    use aws_smithy_http_client::test_util::infallible_client_fn;
    use aws_smithy_lang::ShapeId;
    use aws_smithy_runtime::client::orchestrator::endpoints::StaticUriEndpointResolver;
    use aws_smithy_runtime_api::box_error::BoxError;
    use aws_smithy_runtime_api::client::auth::http::{
        HTTP_API_KEY_AUTH_SCHEME_ID, HTTP_BASIC_AUTH_SCHEME_ID, HTTP_BEARER_AUTH_SCHEME_ID,
    };
    use aws_smithy_runtime_api::client::auth::{
        AuthScheme, AuthSchemeEndpointConfig, AuthSchemeId, Sign,
    };
    use aws_smithy_runtime_api::client::identity::http::{Login, Token};
    use aws_smithy_runtime_api::client::identity::{Identity, SharedIdentityResolver};
    use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
    use aws_smithy_runtime_api::client::runtime_components::{
        GetIdentityResolver, RuntimeComponents, RuntimeComponentsBuilder,
    };
    use aws_smithy_runtime_api::client::runtime_plugin::StaticRuntimePlugin;
    use aws_smithy_types::body::SdkBody;
    use aws_smithy_types::config_bag::ConfigBag;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    // The URI and the `authorization` header of each request a test client sent, in order.
    type Sent = Arc<Mutex<Vec<(String, Option<String>)>>>;

    // Returns a client for `service` of the HTTP auth test model, with `config` layered over the
    // test config.
    fn test_client(service: &str, config: crate::config::Builder) -> (DynamicClient, Sent) {
        let sent = Sent::default();
        let seen = sent.clone();
        let http_client = infallible_client_fn(move |request| {
            let authorization = request
                .headers()
                .get("authorization")
                .map(|value| value.to_str().unwrap().to_owned());
            seen.lock()
                .unwrap()
                .push((request.uri().to_string(), authorization));
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
            .model_file(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/models/http-auth.json"),
            )
            .service(ShapeId::new(service).unwrap())
            .config(
                Config::builder()
                    .behavior_version(BehaviorVersion::latest())
                    .http_client(http_client)
                    .runtime_plugin(endpoint),
            )
            .config(config)
            .build()
            .unwrap();
        (client, sent)
    }

    fn recorded(sent: &Sent) -> Vec<(String, Option<String>)> {
        sent.lock().unwrap().clone()
    }

    fn token(token: &str) -> Token {
        Token::new(token, None)
    }

    #[tokio::test]
    async fn bearer_auth_sends_the_token() {
        let (client, sent) = test_client(
            "smithy.example#Bearer",
            Config::builder().identity_resolver(HTTP_BEARER_AUTH_SCHEME_ID, token("abc")),
        );
        client.call("GetThing").send().await.unwrap();
        assert_eq!(Some("Bearer abc".to_owned()), recorded(&sent)[0].1);
    }

    #[tokio::test]
    async fn basic_auth_sends_the_login() {
        let (client, sent) = test_client(
            "smithy.example#Basic",
            Config::builder()
                .identity_resolver(HTTP_BASIC_AUTH_SCHEME_ID, Login::new("user", "pass", None)),
        );
        client.call("GetThing").send().await.unwrap();
        // `dXNlcjpwYXNz` is `user:pass` in base64.
        assert_eq!(Some("Basic dXNlcjpwYXNz".to_owned()), recorded(&sent)[0].1);
    }

    #[tokio::test]
    async fn api_key_auth_sends_the_key_in_the_modeled_header_with_its_scheme() {
        let (client, sent) = test_client(
            "smithy.example#HeaderKey",
            Config::builder().identity_resolver(HTTP_API_KEY_AUTH_SCHEME_ID, token("key")),
        );
        client.call("GetThing").send().await.unwrap();
        assert_eq!(Some("ApiKey key".to_owned()), recorded(&sent)[0].1);
    }

    #[tokio::test]
    async fn api_key_auth_sends_the_key_in_the_modeled_query_parameter() {
        let (client, sent) = test_client(
            "smithy.example#QueryKey",
            Config::builder().identity_resolver(HTTP_API_KEY_AUTH_SCHEME_ID, token("key")),
        );
        client.call("GetThing").send().await.unwrap();
        let (uri, authorization) = &recorded(&sent)[0];
        assert!(uri.ends_with("?api_key=key"), "{uri}");
        assert_eq!(&None, authorization);
    }

    #[tokio::test]
    async fn a_per_call_identity_resolver_applies_only_to_that_call() {
        let (client, sent) = test_client(
            "smithy.example#Bearer",
            Config::builder().identity_resolver(HTTP_BEARER_AUTH_SCHEME_ID, token("client")),
        );
        client
            .call("GetThing")
            .config_override(
                Config::builder().identity_resolver(HTTP_BEARER_AUTH_SCHEME_ID, token("call")),
            )
            .send()
            .await
            .unwrap();
        client.call("GetThing").send().await.unwrap();
        let authorizations: Vec<_> = recorded(&sent).into_iter().map(|sent| sent.1).collect();
        assert_eq!(
            vec![
                Some("Bearer call".to_owned()),
                Some("Bearer client".to_owned())
            ],
            authorizations
        );
    }

    #[tokio::test]
    async fn a_later_identity_resolver_for_the_same_scheme_wins() {
        let (client, sent) = test_client(
            "smithy.example#Bearer",
            Config::builder()
                .identity_resolver(HTTP_BEARER_AUTH_SCHEME_ID, token("first"))
                .identity_resolver(HTTP_BEARER_AUTH_SCHEME_ID, token("second")),
        );
        client.call("GetThing").send().await.unwrap();
        assert_eq!(Some("Bearer second".to_owned()), recorded(&sent)[0].1);
    }

    #[tokio::test]
    async fn a_call_with_no_identity_for_its_scheme_fails() {
        let (client, sent) = test_client("smithy.example#Bearer", Config::builder());
        assert!(client.call("GetThing").send().await.is_err());
        assert!(recorded(&sent).is_empty());
    }

    // `CustomBearer` is an auth scheme for `httpBearerAuth` that sends `Custom` in the
    // `authorization` header.
    #[derive(Debug)]
    struct CustomBearer;

    impl AuthScheme for CustomBearer {
        fn scheme_id(&self) -> AuthSchemeId {
            HTTP_BEARER_AUTH_SCHEME_ID
        }

        fn identity_resolver(
            &self,
            identity_resolvers: &dyn GetIdentityResolver,
        ) -> Option<SharedIdentityResolver> {
            identity_resolvers.identity_resolver(self.scheme_id())
        }

        fn signer(&self) -> &dyn Sign {
            self
        }
    }

    impl Sign for CustomBearer {
        fn sign_http_request(
            &self,
            request: &mut HttpRequest,
            _identity: &Identity,
            _auth_scheme_endpoint_config: AuthSchemeEndpointConfig<'_>,
            _runtime_components: &RuntimeComponents,
            _config_bag: &ConfigBag,
        ) -> Result<(), BoxError> {
            request.headers_mut().insert("authorization", "Custom");
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_pushed_auth_scheme_replaces_the_modeled_scheme_with_its_id() {
        let (client, sent) = test_client(
            "smithy.example#Bearer",
            Config::builder()
                .push_auth_scheme(CustomBearer)
                .identity_resolver(HTTP_BEARER_AUTH_SCHEME_ID, token("abc")),
        );
        client.call("GetThing").send().await.unwrap();
        assert_eq!(Some("Custom".to_owned()), recorded(&sent)[0].1);
    }
}
