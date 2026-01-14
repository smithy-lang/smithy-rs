/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.auth

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.customizations.TestModels
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rawRust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest

class AuthDecoratorTest {
    private fun codegenScope(runtimeConfig: RuntimeConfig): Array<Pair<String, Any>> =
        arrayOf(
            "ReplayEvent" to
                CargoDependency.smithyRuntime(runtimeConfig)
                    .toDevDependency().withFeature("test-util").toType()
                    .resolve("client::http::test_util::ReplayEvent"),
            "StaticReplayClient" to
                CargoDependency.smithyRuntime(runtimeConfig)
                    .toDevDependency().withFeature("test-util").toType()
                    .resolve("client::http::test_util::StaticReplayClient"),
            "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
        )

    @Test
    fun `register existing auth scheme with no-op signer should disable signing`() {
        clientIntegrationTest(TestModels.allSchemes) { codegenContext, rustCrate ->
            rustCrate.integrationTest("override_auth_scheme_with_no_op_signer") {
                val moduleName = codegenContext.moduleUseName()
                rawRust(
                    """
                    use aws_smithy_runtime_api::{
                        box_error::BoxError,
                        client::{
                            auth::{
                                http::HTTP_API_KEY_AUTH_SCHEME_ID, AuthScheme, AuthSchemeEndpointConfig, AuthSchemeId,
                                Sign,
                            },
                            identity::{Identity, SharedIdentityResolver},
                            identity::http::Token,
                            orchestrator::HttpRequest,
                            runtime_components::{GetIdentityResolver, RuntimeComponents},
                        },
                    };
                    use aws_smithy_types::config_bag::ConfigBag;

                    #[derive(Debug, Default)]
                    struct NoAuthSigner;

                    impl Sign for NoAuthSigner {
                        fn sign_http_request(
                            &self,
                            _request: &mut HttpRequest,
                            _identity: &Identity,
                            _auth_scheme_endpoint_config: AuthSchemeEndpointConfig<'_>,
                            _runtime_components: &RuntimeComponents,
                            _config_bag: &ConfigBag,
                        ) -> Result<(), BoxError> {
                            Ok(())
                        }
                    }

                    #[derive(Debug)]
                    struct ApiKeyAuthSchemeWithNoOpSigner {
                        no_signer: NoAuthSigner
                    }
                    impl Default for ApiKeyAuthSchemeWithNoOpSigner {
                        fn default() -> Self {
                            Self { no_signer: NoAuthSigner }
                        }
                    }
                    impl AuthScheme for ApiKeyAuthSchemeWithNoOpSigner {
                        fn scheme_id(&self) -> AuthSchemeId {
                            HTTP_API_KEY_AUTH_SCHEME_ID
                        }

                        fn identity_resolver(
                            &self,
                            identity_resolvers: &dyn GetIdentityResolver
                        ) -> Option<SharedIdentityResolver> {
                            identity_resolvers.identity_resolver(self.scheme_id())
                        }

                        fn signer(&self) -> &dyn Sign {
                            &self.no_signer
                        }
                    }
                    """,
                )
                Attribute.TokioTest.render(this)
                rustTemplate(
                    """
                    async fn override_auth_scheme_with_no_op_signer_on_service_config() {
                        let http_client = #{StaticReplayClient}::new(
                            vec![#{ReplayEvent}::new(
                                http::Request::builder()
                                    .uri("http://localhost:1234/SomeOperation")
                                    .body(#{SdkBody}::empty())
                                    .unwrap(),
                                http::Response::builder().status(200).body(#{SdkBody}::empty()).unwrap(),
                            )],
                        );
                        let config = $moduleName::Config::builder()
                            .endpoint_url("http://localhost:1234")
                            .api_key(Token::new("some-api-key", None))
                            .push_auth_scheme(ApiKeyAuthSchemeWithNoOpSigner::default())
                            .http_client(http_client.clone())
                            .build();
                        let client = $moduleName::Client::from_conf(config);
                        let _ = client.some_operation()
                            .send()
                            .await
                            .expect("success");
                        http_client.assert_requests_match(&[]);
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )

                Attribute.TokioTest.render(this)
                rustTemplate(
                    """
                    async fn override_auth_scheme_with_no_op_signer_at_operation_level() {
                        let http_client = #{StaticReplayClient}::new(
                            vec![#{ReplayEvent}::new(
                                http::Request::builder()
                                    .uri("http://localhost:1234/SomeOperation")
                                    .body(#{SdkBody}::empty())
                                    .unwrap(),
                                http::Response::builder().status(200).body(#{SdkBody}::empty()).unwrap(),
                            )],
                        );
                        let config = $moduleName::Config::builder()
                            .endpoint_url("http://localhost:1234")
                            .api_key(Token::new("some-api-key", None))
                            .http_client(http_client.clone())
                            .build();
                        let client = $moduleName::Client::from_conf(config);
                        let _ = client.some_operation()
                            .customize()
                            .config_override(
                                $moduleName::Config::builder()
                                    .push_auth_scheme(ApiKeyAuthSchemeWithNoOpSigner::default()),
                            )
                            .send()
                            .await
                            .expect("success");
                        http_client.assert_requests_match(&[]);
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )
            }
        }
    }

    @Test
    fun `register custom auth scheme with identity resolver and signer`() {
        clientIntegrationTest(TestModels.allSchemes) { codegenContext, rustCrate ->
            rustCrate.integrationTest("register_custom_auth_scheme") {
                val moduleName = codegenContext.moduleUseName()
                rawRust(
                    """
                    use aws_smithy_runtime_api::{
                        box_error::BoxError,
                        client::{
                            auth::{
                                AuthScheme, AuthSchemeEndpointConfig, AuthSchemeId, AuthSchemeOption,
                                AuthSchemeOptionsFuture, Sign,
                            },
                            identity::{Identity, IdentityFuture, ResolveIdentity, SharedIdentityResolver},
                            orchestrator::HttpRequest,
                            runtime_components::{GetIdentityResolver, RuntimeComponents},
                        },
                        shared::IntoShared,
                    };
                    use aws_smithy_types::config_bag::ConfigBag;

                    #[derive(Debug)]
                    struct CustomIdentityData(String);

                    #[derive(Debug, Default)]
                    struct CustomSigner;

                    impl Sign for CustomSigner {
                        fn sign_http_request(
                            &self,
                            request: &mut HttpRequest,
                            identity: &Identity,
                            _auth_scheme_endpoint_config: AuthSchemeEndpointConfig<'_>,
                            _runtime_components: &RuntimeComponents,
                            _config_bag: &ConfigBag,
                        ) -> Result<(), BoxError> {
                            let data = &identity.data::<CustomIdentityData>().unwrap().0;
                            let uri = request.uri();
                            request.set_uri(format!("{uri}/{data}")).unwrap();
                            Ok(())
                        }
                    }

                    #[derive(Debug)]
                    struct CustomIdentityResolver;

                    impl ResolveIdentity for CustomIdentityResolver {
                        fn resolve_identity<'a>(
                            &'a self,
                            _runtime_components: &'a RuntimeComponents,
                            _config_bag: &'a ConfigBag,
                        ) -> IdentityFuture<'a> {
                            IdentityFuture::ready(Ok(Identity::new(
                                CustomIdentityData("customidentitydata".to_owned()),
                                None,
                            )))
                        }
                    }

                    #[derive(Debug)]
                    struct CustomAuthScheme {
                        id: AuthSchemeId,
                        signer: CustomSigner,
                        identity_resolver: SharedIdentityResolver,
                    }
                    impl Default for CustomAuthScheme {
                        fn default() -> Self {
                            Self {
                                id: AuthSchemeId::new("custom"),
                                signer: CustomSigner,
                                identity_resolver: CustomIdentityResolver.into_shared(),
                            }
                        }
                    }
                    impl AuthScheme for CustomAuthScheme {
                        fn scheme_id(&self) -> AuthSchemeId {
                            self.id.clone()
                        }

                        fn identity_resolver(
                            &self,
                            _identity_resolvers: &dyn GetIdentityResolver,
                        ) -> Option<SharedIdentityResolver> {
                            Some(self.identity_resolver.clone())
                        }

                        fn signer(&self) -> &dyn Sign {
                            &self.signer
                        }
                    }

                    #[derive(Debug)]
                    struct CustomAuthSchemeResolver;
                    impl $moduleName::config::auth::ResolveAuthScheme for CustomAuthSchemeResolver {
                        fn resolve_auth_scheme<'a>(
                            &'a self,
                            _params: &'a $moduleName::config::auth::Params,
                            _cfg: &'a ConfigBag,
                            _runtime_components: &'a RuntimeComponents,
                        ) -> AuthSchemeOptionsFuture<'a> {
                            AuthSchemeOptionsFuture::ready(Ok(vec![AuthSchemeOption::from(AuthSchemeId::new(
                                "custom",
                            ))]))
                        }
                    }
                    """,
                )
                Attribute.TokioTest.render(this)
                rustTemplate(
                    """
                    async fn register_custom_auth_scheme_on_service_config() {
                        let http_client = #{StaticReplayClient}::new(
                            vec![#{ReplayEvent}::new(
                                http::Request::builder()
                                    .uri("http://localhost:1234/SomeOperation/customidentitydata")
                                    .body(#{SdkBody}::empty())
                                    .unwrap(),
                                http::Response::builder().status(200).body(#{SdkBody}::empty()).unwrap(),
                            )],
                        );
                        let config = $moduleName::Config::builder()
                            .endpoint_url("http://localhost:1234")
                            .push_auth_scheme(CustomAuthScheme::default())
                            .auth_scheme_resolver(CustomAuthSchemeResolver)
                            .http_client(http_client.clone())
                            .build();
                        let client = $moduleName::Client::from_conf(config);
                        let _ = client.some_operation()
                            .send()
                            .await
                            .expect("success");
                        http_client.assert_requests_match(&[]);
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )

                Attribute.TokioTest.render(this)
                rustTemplate(
                    """
                    async fn register_custom_auth_scheme_at_operation_level() {
                        let http_client = #{StaticReplayClient}::new(
                            vec![#{ReplayEvent}::new(
                                http::Request::builder()
                                    .uri("http://localhost:1234/SomeOperation/customidentitydata")
                                    .body(#{SdkBody}::empty())
                                    .unwrap(),
                                http::Response::builder().status(200).body(#{SdkBody}::empty()).unwrap(),
                            )],
                        );
                        let config = $moduleName::Config::builder()
                            .endpoint_url("http://localhost:1234")
                            .http_client(http_client.clone())
                            .build();
                        let client = $moduleName::Client::from_conf(config);
                        let _ = client.some_operation()
                            .customize()
                            .config_override(
                                $moduleName::Config::builder()
                                    .push_auth_scheme(CustomAuthScheme::default())
                                    .auth_scheme_resolver(CustomAuthSchemeResolver)
                            )
                            .send()
                            .await
                            .expect("success");
                        http_client.assert_requests_match(&[]);
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                )

                Attribute.TokioTest.render(this)
                rustTemplate(
                    """
                    async fn no_auth_aware_auth_scheme_option_resolver_via_plugin() {
                        let http_client = #{StaticReplayClient}::new(
                                vec![#{ReplayEvent}::new(
                                    http::Request::builder()
                                    .uri("http://localhost:1234/SomeOperation") // there shouldn't be `customidentitydata` in URI
                                    .body(#{SdkBody}::empty())
                                    .unwrap(),
                                http::Response::builder().status(200).body(#{SdkBody}::empty()).unwrap(),
                            )],
                        );

                        let config = $moduleName::Config::builder()
                            .endpoint_url("http://localhost:1234")
                            .http_client(http_client.clone())
                            .push_auth_scheme(CustomAuthScheme::default())
                            .auth_scheme_resolver(CustomAuthSchemeResolver)
                            // Configures a noAuth-aware auth scheme resolver that appends `NO_AUTH_SCHEME_ID`
                            // to the auth scheme options returned by `CustomAuthSchemeResolver`.
                            .runtime_plugin(#{NoAuthRuntimePluginV2}::new())
                            // Reorders resolved auth scheme options to place `NO_AUTH_SCHEME_ID` first.
                            .auth_scheme_preference([#{NO_AUTH_SCHEME_ID}])
                            .build();
                        let client = $moduleName::Client::from_conf(config);
                        let _ = client.some_operation().send().await.expect("success");
                        http_client.assert_requests_match(&[]);
                    }
                    """,
                    *codegenScope(codegenContext.runtimeConfig),
                    "NO_AUTH_SCHEME_ID" to
                        RuntimeType.smithyRuntime(codegenContext.runtimeConfig)
                            .resolve("client::auth::no_auth::NO_AUTH_SCHEME_ID"),
                    "NoAuthRuntimePluginV2" to
                        RuntimeType.smithyRuntime(codegenContext.runtimeConfig)
                            .resolve("client::auth::no_auth::NoAuthRuntimePluginV2"),
                )
            }
        }
    }
}
