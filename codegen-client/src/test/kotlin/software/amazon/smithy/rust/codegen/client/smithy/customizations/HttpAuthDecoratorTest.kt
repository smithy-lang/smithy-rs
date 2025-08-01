/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rawRust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest

class HttpAuthDecoratorTest {
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
    fun multipleAuthSchemesSchemeSelection() {
        clientIntegrationTest(TestModels.allSchemes) { codegenContext, rustCrate ->
            rustCrate.integrationTest("tests") {
                val moduleName = codegenContext.moduleUseName()
                Attribute.TokioTest.render(this)
                rustTemplate(
                    """
                    async fn use_api_key_auth_when_api_key_provided() {
                        use aws_smithy_runtime_api::client::identity::http::Token;

                        let http_client = #{StaticReplayClient}::new(
                            vec![#{ReplayEvent}::new(
                                http::Request::builder()
                                    .uri("http://localhost:1234/SomeOperation?api_key=some-api-key")
                                    .body(#{SdkBody}::empty())
                                    .unwrap(),
                                http::Response::builder().status(200).body(#{SdkBody}::empty()).unwrap(),
                            )],
                        );

                        let config = $moduleName::Config::builder()
                            .api_key(Token::new("some-api-key", None))
                            .endpoint_url("http://localhost:1234")
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
                    async fn use_basic_auth_when_basic_auth_login_provided() {
                        use aws_smithy_runtime_api::client::identity::http::Login;

                        let http_client = #{StaticReplayClient}::new(
                            vec![#{ReplayEvent}::new(
                                http::Request::builder()
                                    .header("authorization", "Basic c29tZS11c2VyOnNvbWUtcGFzcw==")
                                    .uri("http://localhost:1234/SomeOperation")
                                    .body(#{SdkBody}::empty())
                                    .unwrap(),
                                http::Response::builder().status(200).body(#{SdkBody}::empty()).unwrap(),
                            )],
                        );

                        let config = $moduleName::Config::builder()
                            .basic_auth_login(Login::new("some-user", "some-pass", None))
                            .endpoint_url("http://localhost:1234")
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
            }
        }
    }

    @Test
    fun noAuthCustomAuth() {
        clientIntegrationTest(TestModels.noSchemes) { ctx, rustCrate ->
            rustCrate.integrationTest("custom_auth_scheme_works") {
                val moduleName = ctx.moduleUseName()
                rawRust(
                    """
                    use aws_smithy_runtime_api::client::auth::static_resolver::StaticAuthSchemeOptionResolver;
                    use aws_smithy_runtime_api::client::auth::AuthScheme;
                    use aws_smithy_runtime_api::client::auth::{AuthSchemeId, Sign};
                    use aws_smithy_runtime_api::client::identity::{
                        IdentityFuture, ResolveIdentity, SharedIdentityResolver,
                    };
                    use aws_smithy_runtime_api::box_error::BoxError;
                    use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
                    use aws_smithy_runtime_api::client::runtime_components::{
                        GetIdentityResolver, RuntimeComponentsBuilder,
                    };
                    use aws_smithy_runtime_api::client::identity::Identity;
                    use aws_smithy_runtime_api::client::runtime_plugin::RuntimePlugin;
                    use aws_smithy_runtime_api::client::auth::AuthSchemeEndpointConfig;
                    use aws_smithy_types::config_bag::ConfigBag;
                    use std::borrow::Cow;
                    use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
                    #[derive(Debug)]
                    struct CustomAuthRuntimePlugin {
                        components: RuntimeComponentsBuilder,
                    }

                    impl RuntimePlugin for CustomAuthRuntimePlugin {
                        fn runtime_components(
                            &self,
                            _current_components: &RuntimeComponentsBuilder,
                        ) -> Cow<'_, RuntimeComponentsBuilder> {
                            Cow::Borrowed(&self.components)
                        }
                    }

                    #[derive(Debug)]
                    struct TestAuthScheme { signer: CustomSigner }

                    impl AuthScheme for TestAuthScheme {
                        fn scheme_id(&self) -> AuthSchemeId {
                            AuthSchemeId::new("customauth")
                        }

                        fn identity_resolver(
                            &self,
                            identity_resolvers: &dyn GetIdentityResolver,
                        ) -> Option<SharedIdentityResolver> {
                            identity_resolvers.identity_resolver(self.scheme_id())
                        }

                        fn signer(&self) -> &dyn Sign {
                            &self.signer
                        }
                    }

                    #[derive(Debug, Default)]
                    struct CustomSigner;

                    #[derive(Debug)]
                    struct CustomIdentity(String);

                    impl Sign for CustomSigner {
                        fn sign_http_request(
                            &self,
                            request: &mut HttpRequest,
                            identity: &Identity,
                            // In some advanced use cases, the Smithy `@endpointRuleSet` can
                            // provide additional context in this config:
                            _auth_scheme_endpoint_config: AuthSchemeEndpointConfig<'_>,
                            _runtime_components: &RuntimeComponents,
                            _config_bag: &ConfigBag,
                        ) -> Result<(), BoxError> {
                            // Downcast the identity to our known `Login` type
                            let login = identity
                                .data::<CustomIdentity>()
                                .ok_or("custom auth requires a `Login` identity")?;
                            // Use the login to add an authorization header to the request
                            request.headers_mut().try_append(
                                http::header::AUTHORIZATION,
                                login.0.to_string()
                            )?;
                            Ok(())
                        }
                    }


                    #[derive(Debug)]
                    struct TestResolver;
                    impl ResolveIdentity for TestResolver {
                        fn resolve_identity<'a>(
                            &'a self,
                            _runtime_components: &'a RuntimeComponents,
                            _config_bag: &'a ConfigBag,
                        ) -> IdentityFuture<'a> {
                            IdentityFuture::ready(Ok(Identity::new(CustomIdentity("password".to_string()), None)))
                        }
                    }

                    impl CustomAuthRuntimePlugin {
                        pub fn new() -> Self {
                            let scheme_id = AuthSchemeId::new("customauth");
                            Self {
                                components: RuntimeComponentsBuilder::new("test-auth-scheme")
                                    // Register our auth scheme
                                    .with_auth_scheme(TestAuthScheme { signer: CustomSigner })
                                    // Register our identity resolver with our auth scheme ID
                                    .with_identity_resolver(
                                        // This scheme ID needs to match the scheme ID returned in the auth scheme implementation
                                        scheme_id.clone(),
                                        TestResolver,
                                    )
                                    // Set the auth scheme option resolver to always use our basic auth auth scheme
                                    .with_auth_scheme_option_resolver(Some(StaticAuthSchemeOptionResolver::new(vec![
                                        scheme_id,
                                    ]))),
                            }
                        }
                    }

                    #[test]
                    fn compile() {}

                    """,
                )
                Attribute.TokioTest.render(this)
                rustTemplate(
                    """
                    async fn apply_custom_auth_scheme() {
                        let (_guard, _rx) = #{capture_test_logs}();
                        let http_client = #{StaticReplayClient}::new(
                            vec![#{ReplayEvent}::new(
                                http::Request::builder()
                                    .header("authorization", "password")
                                    .uri("http://localhost:1234/SomeOperation")
                                    .body(#{SdkBody}::empty())
                                    .unwrap(),
                                http::Response::builder().status(200).body(#{SdkBody}::empty()).unwrap(),
                            )],
                        );
                        let config = $moduleName::Config::builder()
                            .endpoint_url("http://localhost:1234")
                            .http_client(http_client.clone())
                            .runtime_plugin(CustomAuthRuntimePlugin::new())
                            .build();
                        let client = $moduleName::Client::from_conf(config);
                        let _req = dbg!(client.some_operation()
                            .send()
                            .await).expect("request should succeed");

                        http_client.assert_requests_match(&[]);
                    }
                    """,
                    "capture_test_logs" to
                        CargoDependency.smithyRuntimeTestUtil(ctx.runtimeConfig).toType()
                            .resolve("test_util::capture_test_logs::capture_test_logs"),
                    *codegenScope(ctx.runtimeConfig),
                )
            }
        }
    }

    @Test
    fun apiKeyInQueryString() {
        clientIntegrationTest(TestModels.apiKeyInQueryString) { codegenContext, rustCrate ->
            rustCrate.integrationTest("api_key_applied_to_query_string") {
                val moduleName = codegenContext.moduleUseName()
                Attribute.TokioTest.render(this)
                rustTemplate(
                    """
                    async fn api_key_applied_to_query_string() {
                        use aws_smithy_runtime_api::client::identity::http::Token;

                        let http_client = #{StaticReplayClient}::new(
                            vec![#{ReplayEvent}::new(
                                http::Request::builder()
                                    .uri("http://localhost:1234/SomeOperation?api_key=some-api-key")
                                    .body(#{SdkBody}::empty())
                                    .unwrap(),
                                http::Response::builder().status(200).body(#{SdkBody}::empty()).unwrap(),
                            )],
                        );

                        let config = $moduleName::Config::builder()
                            .api_key(Token::new("some-api-key", None))
                            .endpoint_url("http://localhost:1234")
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
            }
        }
    }

    @Test
    fun apiKeyInHeaders() {
        clientIntegrationTest(TestModels.apiKeyInHeaders) { codegenContext, rustCrate ->
            rustCrate.integrationTest("api_key_applied_to_headers") {
                val moduleName = codegenContext.moduleUseName()
                Attribute.TokioTest.render(this)
                rustTemplate(
                    """
                    async fn api_key_applied_to_headers() {
                        use aws_smithy_runtime_api::client::identity::http::Token;

                        let http_client = #{StaticReplayClient}::new(
                            vec![#{ReplayEvent}::new(
                                http::Request::builder()
                                    .header("authorization", "ApiKey some-api-key")
                                    .uri("http://localhost:1234/SomeOperation")
                                    .body(#{SdkBody}::empty())
                                    .unwrap(),
                                http::Response::builder().status(200).body(#{SdkBody}::empty()).unwrap(),
                            )],
                        );

                        let config = $moduleName::Config::builder()
                            .api_key(Token::new("some-api-key", None))
                            .endpoint_url("http://localhost:1234")
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
            }
        }
    }

    @Test
    fun basicAuth() {
        clientIntegrationTest(TestModels.basicAuth) { codegenContext, rustCrate ->
            rustCrate.integrationTest("basic_auth") {
                val moduleName = codegenContext.moduleUseName()
                Attribute.TokioTest.render(this)
                rustTemplate(
                    """
                    async fn basic_auth() {
                        use aws_smithy_runtime_api::client::identity::http::Login;

                        let http_client = #{StaticReplayClient}::new(
                            vec![#{ReplayEvent}::new(
                                http::Request::builder()
                                    .header("authorization", "Basic c29tZS11c2VyOnNvbWUtcGFzcw==")
                                    .uri("http://localhost:1234/SomeOperation")
                                    .body(#{SdkBody}::empty())
                                    .unwrap(),
                                http::Response::builder().status(200).body(#{SdkBody}::empty()).unwrap(),
                            )],
                        );

                        let config = $moduleName::Config::builder()
                            .basic_auth_login(Login::new("some-user", "some-pass", None))
                            .endpoint_url("http://localhost:1234")
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
            }
        }
    }

    @Test
    fun bearerAuth() {
        clientIntegrationTest(TestModels.bearerAuth) { codegenContext, rustCrate ->
            rustCrate.integrationTest("bearer_auth") {
                val moduleName = codegenContext.moduleUseName()
                Attribute.TokioTest.render(this)
                rustTemplate(
                    """
                    async fn basic_auth() {
                        use aws_smithy_runtime_api::client::identity::http::Token;

                        let http_client = #{StaticReplayClient}::new(
                            vec![#{ReplayEvent}::new(
                                http::Request::builder()
                                    .header("authorization", "Bearer some-token")
                                    .uri("http://localhost:1234/SomeOperation")
                                    .body(#{SdkBody}::empty())
                                    .unwrap(),
                                http::Response::builder().status(200).body(#{SdkBody}::empty()).unwrap(),
                            )],
                        );

                        let config = $moduleName::Config::builder()
                            .bearer_token(Token::new("some-token", None))
                            .endpoint_url("http://localhost:1234")
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
            }
        }
    }

    @Test
    fun optionalAuth() {
        clientIntegrationTest(TestModels.optionalAuth) { codegenContext, rustCrate ->
            rustCrate.integrationTest("optional_auth") {
                val moduleName = codegenContext.moduleUseName()
                Attribute.TokioTest.render(this)
                rustTemplate(
                    """
                    async fn optional_auth() {
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
            }
        }
    }
}

internal object TestModels {
    val allSchemes =
        """
        namespace test

        use aws.api#service
        use aws.protocols#restJson1

        @service(sdkId: "Test Api Key Auth")
        @restJson1
        @httpApiKeyAuth(name: "api_key", in: "query")
        @httpBasicAuth
        @httpBearerAuth
        @httpDigestAuth
        @auth([httpApiKeyAuth, httpBasicAuth, httpBearerAuth, httpDigestAuth])
        service TestService {
            version: "2023-01-01",
            operations: [SomeOperation]
        }

        structure SomeOutput {
            someAttribute: Long,
            someVal: String
        }

        @http(uri: "/SomeOperation", method: "GET")
        operation SomeOperation {
            output: SomeOutput
        }
        """.asSmithyModel()

    val noSchemes =
        """
        namespace test

        use aws.api#service
        use aws.protocols#restJson1

        @service(sdkId: "Test Api Key Auth")
        @restJson1
        service TestService {
            version: "2023-01-01",
            operations: [SomeOperation]
        }

        structure SomeOutput {
            someAttribute: Long,
            someVal: String
        }

        @http(uri: "/SomeOperation", method: "GET")
        operation SomeOperation {
            output: SomeOutput
        }""".asSmithyModel()

    val apiKeyInQueryString =
        """
        namespace test

        use aws.api#service
        use aws.protocols#restJson1

        @service(sdkId: "Test Api Key Auth")
        @restJson1
        @httpApiKeyAuth(name: "api_key", in: "query")
        @auth([httpApiKeyAuth])
        service TestService {
            version: "2023-01-01",
            operations: [SomeOperation]
        }

        structure SomeOutput {
            someAttribute: Long,
            someVal: String
        }

        @http(uri: "/SomeOperation", method: "GET")
        operation SomeOperation {
            output: SomeOutput
        }
        """.asSmithyModel()

    val apiKeyInHeaders =
        """
        namespace test

        use aws.api#service
        use aws.protocols#restJson1

        @service(sdkId: "Test Api Key Auth")
        @restJson1
        @httpApiKeyAuth(name: "authorization", in: "header", scheme: "ApiKey")
        @auth([httpApiKeyAuth])
        service TestService {
            version: "2023-01-01",
            operations: [SomeOperation]
        }

        structure SomeOutput {
            someAttribute: Long,
            someVal: String
        }

        @http(uri: "/SomeOperation", method: "GET")
        operation SomeOperation {
            output: SomeOutput
        }
        """.asSmithyModel()

    val basicAuth =
        """
        namespace test

        use aws.api#service
        use aws.protocols#restJson1

        @service(sdkId: "Test Api Key Auth")
        @restJson1
        @httpBasicAuth
        @auth([httpBasicAuth])
        service TestService {
            version: "2023-01-01",
            operations: [SomeOperation]
        }

        structure SomeOutput {
            someAttribute: Long,
            someVal: String
        }

        @http(uri: "/SomeOperation", method: "GET")
        operation SomeOperation {
            output: SomeOutput
        }
        """.asSmithyModel()

    val bearerAuth =
        """
        namespace test

        use aws.api#service
        use aws.protocols#restJson1

        @service(sdkId: "Test Api Key Auth")
        @restJson1
        @httpBearerAuth
        @auth([httpBearerAuth])
        service TestService {
            version: "2023-01-01",
            operations: [SomeOperation]
        }

        structure SomeOutput {
            someAttribute: Long,
            someVal: String
        }

        @http(uri: "/SomeOperation", method: "GET")
        operation SomeOperation {
            output: SomeOutput
        }
        """.asSmithyModel()

    val optionalAuth =
        """
        namespace test

        use aws.api#service
        use aws.protocols#restJson1

        @service(sdkId: "Test Api Key Auth")
        @restJson1
        @httpBearerAuth
        @auth([httpBearerAuth])
        service TestService {
            version: "2023-01-01",
            operations: [SomeOperation]
        }

        structure SomeOutput {
            someAttribute: Long,
            someVal: String
        }

        @http(uri: "/SomeOperation", method: "GET")
        @optionalAuth
        operation SomeOperation {
            output: SomeOutput
        }
        """.asSmithyModel()
}
