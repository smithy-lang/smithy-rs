/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// The auth schemes of an operation, read from the model's auth traits, and the default auth scheme
// resolver that returns them.

use aws_smithy_lang::shape::{OperationView, ServiceView};
use aws_smithy_lang::{Model, Node, ShapeId};
use aws_smithy_runtime::client::auth::no_auth::NO_AUTH_SCHEME_ID;
use aws_smithy_runtime_api::client::auth::{
    AuthSchemeId, AuthSchemeOption, AuthSchemeOptionResolverParams, AuthSchemeOptionsFuture,
    ResolveAuthSchemeOptions,
};
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_types::config_bag::ConfigBag;
use std::borrow::Cow;

/// The parameters an auth scheme resolver gets for one call.
///
/// A resolver set with
/// [`auth_scheme_resolver`](crate::config::Builder::auth_scheme_resolver) reads them from its
/// `AuthSchemeOptionResolverParams` with `params.get::<Params>()`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Params {
    operation_name: String,
    auth_schemes: Vec<AuthSchemeId>,
}

impl Params {
    /// Returns the shape name of the called operation, such as `"GetForecast"`.
    pub fn operation_name(&self) -> &str {
        &self.operation_name
    }

    /// Returns the operation's auth schemes from the model, in the model's order of priority.
    ///
    /// The list follows the operation's `@auth` trait, or else the service's `@auth` trait. With
    /// neither trait, the list has every auth trait of the service, sorted by shape ID. An
    /// operation with `@optionalAuth` gets `noAuth` last. An operation with no auth scheme gets
    /// only `noAuth`.
    ///
    /// The ID of each scheme is the shape name of its trait, such as `httpBearerAuth` for
    /// `@httpBearerAuth`, or `sigv4` for `@aws.auth#sigv4`.
    pub fn auth_schemes(&self) -> &[AuthSchemeId] {
        &self.auth_schemes
    }
}

// Returns the auth scheme parameters of `operation`, with its auth schemes from the model.
pub(crate) fn params(
    model: &Model,
    service: &ServiceView<'_>,
    operation: &OperationView<'_>,
) -> Params {
    Params {
        operation_name: operation.id().name().to_owned(),
        auth_schemes: effective_auth(model, service, operation),
    }
}

// Returns the auth schemes of `operation`, as Smithy's `ServiceIndex` returns them in the
// `NO_AUTH_AWARE` mode.
//
// TODO(aws-smithy-lang): Use a `ServiceIndex` for the effective auth schemes, once the crate has
// one.
fn effective_auth(
    model: &Model,
    service: &ServiceView<'_>,
    operation: &OperationView<'_>,
) -> Vec<AuthSchemeId> {
    let mut traits = auth_trait_values(service, operation.get_trait(&AUTH))
        .or_else(|| auth_trait_values(service, service.get_trait(&AUTH)))
        .unwrap_or_else(|| service_auth_traits(model, service));
    if traits.is_empty() || operation.has_trait(&OPTIONAL_AUTH) {
        traits.push(NO_AUTH.clone());
    }
    traits.iter().map(scheme_id).collect()
}

// Returns the values of an `@auth` trait that the service applies, in the trait's order. Returns
// `None` when the trait is absent.
fn auth_trait_values(service: &ServiceView<'_>, auth: Option<&Node>) -> Option<Vec<ShapeId>> {
    let values = auth?.as_array().unwrap_or_default();
    Some(
        values
            .iter()
            .filter_map(|value| value.as_str())
            .filter_map(|value| ShapeId::new(value).ok())
            .filter(|id| service.has_trait(id))
            .collect(),
    )
}

// Returns the service's auth traits, sorted by shape ID as Smithy sorts them: case-insensitively
// first, then case-sensitively to break ties. An auth trait is a trait whose shape has
// `@authDefinition`.
fn service_auth_traits(model: &Model, service: &ServiceView<'_>) -> Vec<ShapeId> {
    let mut traits: Vec<ShapeId> = service
        .traits()
        .ids()
        .filter(|id| {
            model
                .get_shape(id)
                .and_then(|shape| shape.as_shape())
                .is_some_and(|shape| shape.has_trait(&AUTH_DEFINITION))
        })
        .cloned()
        .collect();
    traits.sort_by(|a, b| {
        let (a, b) = (a.as_str(), b.as_str());
        a.to_lowercase().cmp(&b.to_lowercase()).then(a.cmp(b))
    });
    traits
}

// The scheme ID of an auth trait is the trait's shape name.
fn scheme_id(auth_trait: &ShapeId) -> AuthSchemeId {
    if *auth_trait == NO_AUTH {
        return NO_AUTH_SCHEME_ID;
    }
    AuthSchemeId::from(Cow::Owned(auth_trait.name().to_owned()))
}

const AUTH: ShapeId = ShapeId::from_static("smithy.api#auth");
const AUTH_DEFINITION: ShapeId = ShapeId::from_static("smithy.api#authDefinition");
const NO_AUTH: ShapeId = ShapeId::from_static("smithy.api#noAuth");
const OPTIONAL_AUTH: ShapeId = ShapeId::from_static("smithy.api#optionalAuth");

// The default auth scheme resolver. It returns the operation's auth schemes from the model, so the
// orchestrator uses the first scheme it has an implementation and an identity for.
#[derive(Debug)]
pub(crate) struct ModeledAuthSchemeResolver;

impl ResolveAuthSchemeOptions for ModeledAuthSchemeResolver {
    fn resolve_auth_scheme_options_v2<'a>(
        &'a self,
        params: &'a AuthSchemeOptionResolverParams,
        _cfg: &'a ConfigBag,
        _runtime_components: &'a RuntimeComponents,
    ) -> AuthSchemeOptionsFuture<'a> {
        let options = params
            .get::<Params>()
            .map(|params| {
                params
                    .auth_schemes
                    .iter()
                    .cloned()
                    .map(AuthSchemeOption::from)
                    .collect()
            })
            .ok_or_else(|| "the auth scheme params are not the dynamic client's `Params`".into());
        AuthSchemeOptionsFuture::ready(options)
    }
}

#[cfg(test)]
mod tests {
    use super::{effective_auth, Params};
    use crate::config::{AuthSchemeId, BehaviorVersion, Config};
    use crate::DynamicClient;
    use aws_smithy_http_client::test_util::infallible_client_fn;
    use aws_smithy_lang::{Model, ShapeId};
    use aws_smithy_runtime::client::auth::no_auth::NO_AUTH_SCHEME_ID;
    use aws_smithy_runtime::client::orchestrator::endpoints::StaticUriEndpointResolver;
    use aws_smithy_runtime_api::box_error::BoxError;
    use aws_smithy_runtime_api::client::auth::http::HTTP_BEARER_AUTH_SCHEME_ID;
    use aws_smithy_runtime_api::client::auth::{
        AuthScheme, AuthSchemeEndpointConfig, AuthSchemeOption, AuthSchemeOptionResolverParams,
        AuthSchemeOptionsFuture, ResolveAuthSchemeOptions, Sign,
    };
    use aws_smithy_runtime_api::client::identity::http::Token;
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

    fn model_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/models/auth.json")
    }

    fn id(id: &str) -> ShapeId {
        ShapeId::new(id).unwrap()
    }

    fn scheme_ids(names: &[&'static str]) -> Vec<AuthSchemeId> {
        names.iter().map(|name| AuthSchemeId::new(name)).collect()
    }

    // Returns the auth schemes of `operation` in `service` of the auth test model.
    fn auth_schemes(service: &str, operation: &str) -> Vec<AuthSchemeId> {
        let model = Model::from_json_file(model_path()).unwrap();
        let service_view = model
            .get_shape(&id(service))
            .and_then(|shape| shape.as_shape())
            .and_then(|shape| shape.as_service())
            .unwrap();
        let operation_view = model
            .get_shape(&id(operation))
            .and_then(|shape| shape.as_shape())
            .and_then(|shape| shape.as_operation())
            .unwrap();
        effective_auth(&model, &service_view, &operation_view)
    }

    #[test]
    fn an_operation_with_no_auth_trait_gets_every_service_auth_trait_sorted_by_shape_id() {
        assert_eq!(
            scheme_ids(&["httpBasicAuth", "httpBearerAuth", "customAuth", "ZetaAuth"]),
            auth_schemes("smithy.example#Gizmos", "smithy.example#GetGizmo")
        );
    }

    #[test]
    fn an_operation_auth_trait_keeps_its_order_and_only_the_service_schemes() {
        assert_eq!(
            scheme_ids(&["httpBearerAuth", "httpBasicAuth"]),
            auth_schemes("smithy.example#Gizmos", "smithy.example#PutGizmo")
        );
    }

    #[test]
    fn optional_auth_adds_no_auth_last() {
        assert_eq!(
            scheme_ids(&[
                "httpBasicAuth",
                "httpBearerAuth",
                "customAuth",
                "ZetaAuth",
                "noAuth"
            ]),
            auth_schemes("smithy.example#Gizmos", "smithy.example#ListGizmos")
        );
    }

    #[test]
    fn an_empty_operation_auth_trait_gives_only_no_auth() {
        assert_eq!(
            vec![NO_AUTH_SCHEME_ID],
            auth_schemes("smithy.example#Gizmos", "smithy.example#DeleteGizmo")
        );
    }

    #[test]
    fn the_service_auth_trait_applies_to_an_operation_without_one() {
        assert_eq!(
            scheme_ids(&["httpBearerAuth"]),
            auth_schemes("smithy.example#Gadgets", "smithy.example#GetGadget")
        );
        assert_eq!(
            scheme_ids(&["httpBasicAuth"]),
            auth_schemes("smithy.example#Gadgets", "smithy.example#PutGadget")
        );
    }

    #[test]
    fn a_service_with_no_auth_trait_gives_only_no_auth() {
        assert_eq!(
            vec![NO_AUTH_SCHEME_ID],
            auth_schemes("smithy.example#Gears", "smithy.example#GetGear")
        );
    }

    // `TestBearer` is an auth scheme for `httpBearerAuth` that adds an `x-signed-by` header.
    #[derive(Debug)]
    struct TestBearer;

    impl AuthScheme for TestBearer {
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

    impl Sign for TestBearer {
        fn sign_http_request(
            &self,
            request: &mut HttpRequest,
            _identity: &Identity,
            _auth_scheme_endpoint_config: AuthSchemeEndpointConfig<'_>,
            _runtime_components: &RuntimeComponents,
            _config_bag: &ConfigBag,
        ) -> Result<(), BoxError> {
            request
                .headers_mut()
                .insert("x-signed-by", "httpBearerAuth");
            Ok(())
        }
    }

    // `NoAuthOnly` is an auth scheme resolver that returns only `noAuth`.
    #[derive(Debug)]
    struct NoAuthOnly;

    impl ResolveAuthSchemeOptions for NoAuthOnly {
        fn resolve_auth_scheme_options_v2<'a>(
            &'a self,
            params: &'a AuthSchemeOptionResolverParams,
            _cfg: &'a ConfigBag,
            _runtime_components: &'a RuntimeComponents,
        ) -> AuthSchemeOptionsFuture<'a> {
            assert!(params.get::<Params>().is_some());
            AuthSchemeOptionsFuture::ready(Ok(vec![AuthSchemeOption::from(NO_AUTH_SCHEME_ID)]))
        }
    }

    // The `x-signed-by` header of each request a test client sent, in order.
    type SignedBy = Arc<Mutex<Vec<Option<String>>>>;

    // Returns a client for `service` of the auth test model, with `config` layered over the test
    // config. The test config has the `TestBearer` scheme and a token for it.
    fn test_client(service: &str, config: crate::config::Builder) -> (DynamicClient, SignedBy) {
        let signed_by = SignedBy::default();
        let seen = signed_by.clone();
        let http_client = infallible_client_fn(move |request| {
            let header = request.headers().get("x-signed-by");
            seen.lock()
                .unwrap()
                .push(header.map(|value| value.to_str().unwrap().to_owned()));
            http_1x::Response::builder()
                .status(200)
                .body(SdkBody::from("{}"))
                .unwrap()
        });
        // TODO(dynamic-client): Remove the endpoint resolver once the endpoint comes from the
        // model.
        let test_plugin = StaticRuntimePlugin::new().with_runtime_components(
            RuntimeComponentsBuilder::new("TestAuth")
                .with_endpoint_resolver(Some(StaticUriEndpointResolver::uri("https://example.com")))
                .with_auth_scheme(TestBearer)
                .with_identity_resolver(HTTP_BEARER_AUTH_SCHEME_ID, Token::new("token", None)),
        );
        let client = DynamicClient::builder()
            .model_file(model_path())
            .service(id(service))
            .config(
                Config::builder()
                    .behavior_version(BehaviorVersion::latest())
                    .http_client(http_client)
                    .runtime_plugin(test_plugin),
            )
            .config(config)
            .build()
            .unwrap();
        (client, signed_by)
    }

    const GIZMOS: &str = "smithy.example#Gizmos";

    fn recorded(signed_by: &SignedBy) -> Vec<Option<String>> {
        signed_by.lock().unwrap().clone()
    }

    fn bearer() -> Option<String> {
        Some("httpBearerAuth".to_owned())
    }

    #[tokio::test]
    async fn a_call_uses_the_first_modeled_scheme_the_client_has() {
        let (client, signed_by) = test_client(GIZMOS, Config::builder());
        client.call("ListGizmos").send().await.unwrap();
        assert_eq!(vec![bearer()], recorded(&signed_by));
    }

    #[tokio::test]
    async fn the_auth_scheme_preference_reorders_the_modeled_schemes() {
        let (client, signed_by) = test_client(
            GIZMOS,
            Config::builder().auth_scheme_preference([NO_AUTH_SCHEME_ID]),
        );
        client.call("ListGizmos").send().await.unwrap();
        assert_eq!(vec![None], recorded(&signed_by));
    }

    #[tokio::test]
    async fn a_per_call_auth_scheme_preference_applies_only_to_that_call() {
        let (client, signed_by) = test_client(GIZMOS, Config::builder());
        client
            .call("ListGizmos")
            .config_override(Config::builder().auth_scheme_preference([NO_AUTH_SCHEME_ID]))
            .send()
            .await
            .unwrap();
        client.call("ListGizmos").send().await.unwrap();
        assert_eq!(vec![None, bearer()], recorded(&signed_by));
    }

    #[tokio::test]
    async fn the_auth_scheme_resolver_wins_over_the_modeled_schemes() {
        let (client, signed_by) =
            test_client(GIZMOS, Config::builder().auth_scheme_resolver(NoAuthOnly));
        client.call("GetGizmo").send().await.unwrap();
        assert_eq!(vec![None], recorded(&signed_by));
    }

    #[tokio::test]
    async fn allow_no_auth_sends_a_call_that_has_no_identity_for_its_schemes() {
        // `PutGadget` has only `httpBasicAuth`, and the client has no identity for it.
        let (client, _) = test_client("smithy.example#Gadgets", Config::builder());
        assert!(client.call("PutGadget").send().await.is_err());
        let (client, signed_by) =
            test_client("smithy.example#Gadgets", Config::builder().allow_no_auth());
        client.call("PutGadget").send().await.unwrap();
        assert_eq!(vec![None], recorded(&signed_by));
    }
}
