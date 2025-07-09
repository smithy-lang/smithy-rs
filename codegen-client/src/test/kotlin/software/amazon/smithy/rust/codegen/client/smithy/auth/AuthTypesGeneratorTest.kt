/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.auth

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.customizations.HttpAuthDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customizations.NoAuthDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.CombinedClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.testutil.testClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest

class AuthTypesGeneratorTest {
    val model =
        """
        namespace com.test

        use aws.auth#unsignedPayload

        @httpBearerAuth
        @httpApiKeyAuth(name: "X-Api-Key", in: "header")
        @httpBasicAuth
        @auth([httpApiKeyAuth, httpBasicAuth])
        service Test {
            version: "1.0.0",
            operations: [
                GetFooServiceDefault,
                GetFooOpOverride,
                GetFooAnonymous,
            ]
        }

        operation GetFooServiceDefault {}

        @auth([httpBasicAuth, httpBearerAuth])
        operation GetFooOpOverride{}

        @auth([])
        operation GetFooAnonymous{}
        """.asSmithyModel(smithyVersion = "2.0")

    @Test
    fun `basic test for default auth scheme resolver`() {
        val ctx =
            testClientCodegenContext(
                model,
                rootDecorator =
                    CombinedClientCodegenDecorator(
                        listOf(
                            NoAuthDecorator(),
                            HttpAuthDecorator(),
                        ),
                    ),
            )
        val sut = AuthTypesGenerator(ctx)
        val project = TestWorkspace.testProject()
        project.withModule(RustModule.private("default_auth_scheme_resolver").cfgTest()) {
            val codegenScope =
                arrayOf(
                    "ConfigBag" to
                        CargoDependency.smithyTypes(ctx.runtimeConfig)
                            .copy(features = setOf("test-util"), scope = DependencyScope.Dev).toType()
                            .resolve("config_bag::ConfigBag"),
                    "DefaultAuthSchemeResolver" to sut.defaultAuthSchemeResolver(),
                    "Params" to AuthSchemeParamsGenerator(ctx).paramsStruct(),
                    "RuntimeComponentsBuilder" to
                        CargoDependency.smithyRuntimeApiClient(ctx.runtimeConfig)
                            .copy(features = setOf("test-util"), scope = DependencyScope.Dev).toType()
                            .resolve("client::runtime_components::RuntimeComponentsBuilder"),
                    "ServiceSpecificResolver" to sut.serviceSpecificResolveAuthSchemeTrait(),
                )
            rustTemplate("use #{ServiceSpecificResolver};", *codegenScope)
            tokioTest("should_return_service_defaults") {
                rustTemplate(
                    """
                    let sut = #{DefaultAuthSchemeResolver}::default();
                    let params = #{Params}::builder()
                        .operation_name("GetFooServiceDefault")
                        .build()
                        .unwrap();
                    let cfg = #{ConfigBag}::base();
                    let rc =
                        #{RuntimeComponentsBuilder}::for_tests()
                        .build()
                        .unwrap();
                    let actual = sut
                        .resolve_auth_scheme(&params, &cfg, &rc)
                        .await
                        .unwrap();
                    let actual = actual
                        .iter()
                        .map(|opt| opt.scheme_id().inner())
                        .collect::<Vec<_>>();
                    assert_eq!(vec!["http-api-key-auth", "http-basic-auth"], actual);
                    """,
                    *codegenScope,
                )
            }

            tokioTest("should_return_operation_overrides") {
                rustTemplate(
                    """
                    let sut = #{DefaultAuthSchemeResolver}::default();
                    let params = #{Params}::builder()
                        .operation_name("GetFooOpOverride")
                        .build()
                        .unwrap();
                    let cfg = #{ConfigBag}::base();
                    let rc =
                        #{RuntimeComponentsBuilder}::for_tests()
                        .build()
                        .unwrap();
                    let actual = sut
                        .resolve_auth_scheme(&params, &cfg, &rc)
                        .await
                        .unwrap();
                    let actual = actual
                        .iter()
                        .map(|opt| opt.scheme_id().inner())
                        .collect::<Vec<_>>();
                    assert_eq!(vec!["http-basic-auth", "http-bearer-auth"], actual);
                    """,
                    *codegenScope,
                )
            }
        }
        project.compileAndTest()
    }

    @Test
    fun `auth scheme preference`() {
        val ctx =
            testClientCodegenContext(
                model,
                rootDecorator =
                    CombinedClientCodegenDecorator(
                        listOf(
                            NoAuthDecorator(),
                            HttpAuthDecorator(),
                        ),
                    ),
            )
        val sut = AuthTypesGenerator(ctx)
        val project = TestWorkspace.testProject()
        project.withModule(RustModule.private("auth_scheme_preference").cfgTest()) {
            val smithyApiClient =
                CargoDependency.smithyRuntimeApiClient(ctx.runtimeConfig)
                    .copy(features = setOf("http-auth", "test-util"), scope = DependencyScope.Dev).toType()
            val codegenScope =
                arrayOf(
                    "AuthSchemeId" to smithyApiClient.resolve("client::auth::AuthSchemeId"),
                    "ConfigBag" to
                        CargoDependency.smithyTypes(ctx.runtimeConfig)
                            .copy(features = setOf("test-util"), scope = DependencyScope.Dev).toType()
                            .resolve("config_bag::ConfigBag"),
                    "DefaultAuthSchemeResolver" to sut.defaultAuthSchemeResolver(),
                    "HTTP_API_KEY_AUTH_SCHEME_ID" to smithyApiClient.resolve("client::auth::http::HTTP_API_KEY_AUTH_SCHEME_ID"),
                    "HTTP_BASIC_AUTH_SCHEME_ID" to smithyApiClient.resolve("client::auth::http::HTTP_BASIC_AUTH_SCHEME_ID"),
                    "HTTP_BEARER_AUTH_SCHEME_ID" to
                        smithyApiClient
                            .resolve("client::auth::http::HTTP_BEARER_AUTH_SCHEME_ID"),
                    "Params" to AuthSchemeParamsGenerator(ctx).paramsStruct(),
                    "RuntimeComponentsBuilder" to
                        smithyApiClient
                            .resolve("client::runtime_components::RuntimeComponentsBuilder"),
                    "ServiceSpecificResolver" to sut.serviceSpecificResolveAuthSchemeTrait(),
                )
            rustTemplate("use #{ServiceSpecificResolver};", *codegenScope)
            tokioTest("test_auth_scheme_preference") {
                rustTemplate(
                    """
                    struct TestCase<'a> {
                        auth_scheme_params: &'a #{Params},
                        preference_list: &'a[#{AuthSchemeId}],
                        expected_resolved_auths: Vec<#{AuthSchemeId}>,
                    }

                    let get_foo_service_default_params = #{Params}::builder()
                       .operation_name("GetFooServiceDefault")
                       .build()
                       .unwrap();
                    let get_foo_op_override_params = #{Params}::builder()
                       .operation_name("GetFooOpOverride")
                       .build()
                       .unwrap();
                    let cfg = #{ConfigBag}::base();
                    let rc =
                       #{RuntimeComponentsBuilder}::for_tests()
                       .build()
                       .unwrap();

                    for test_case in [
                       TestCase {
                           auth_scheme_params: &get_foo_service_default_params,
                           preference_list: &[],
                           expected_resolved_auths: vec![#{HTTP_API_KEY_AUTH_SCHEME_ID}, #{HTTP_BASIC_AUTH_SCHEME_ID}],
                       },
                       TestCase {
                           auth_scheme_params: &get_foo_service_default_params,
                           preference_list: &["bogus".into()],
                           expected_resolved_auths: vec![#{HTTP_API_KEY_AUTH_SCHEME_ID}, #{HTTP_BASIC_AUTH_SCHEME_ID}],
                       },
                       TestCase {
                           auth_scheme_params: &get_foo_service_default_params,
                           preference_list: &[#{HTTP_BASIC_AUTH_SCHEME_ID}],
                           expected_resolved_auths: vec![#{HTTP_BASIC_AUTH_SCHEME_ID}, #{HTTP_API_KEY_AUTH_SCHEME_ID}],
                       },
                       TestCase {
                           auth_scheme_params: &get_foo_service_default_params,
                           preference_list: &[#{HTTP_BASIC_AUTH_SCHEME_ID},  #{HTTP_API_KEY_AUTH_SCHEME_ID}],
                           expected_resolved_auths: vec![#{HTTP_BASIC_AUTH_SCHEME_ID}, #{HTTP_API_KEY_AUTH_SCHEME_ID}],
                       },
                       TestCase {
                           auth_scheme_params: &get_foo_op_override_params,
                           preference_list: &[#{HTTP_BEARER_AUTH_SCHEME_ID}],
                           expected_resolved_auths: vec![#{HTTP_BEARER_AUTH_SCHEME_ID}, #{HTTP_BASIC_AUTH_SCHEME_ID}],
                       },
                       TestCase {
                           auth_scheme_params: &get_foo_op_override_params,
                           preference_list: &[#{HTTP_API_KEY_AUTH_SCHEME_ID}],
                           expected_resolved_auths: vec![#{HTTP_BASIC_AUTH_SCHEME_ID}, #{HTTP_BEARER_AUTH_SCHEME_ID}],
                       },
                    ].into_iter() {
                       let sut = #{DefaultAuthSchemeResolver}::default()
                           .with_auth_scheme_preference(test_case.preference_list);
                       let actual = sut
                           .resolve_auth_scheme(&test_case.auth_scheme_params, &cfg, &rc)
                           .await
                           .unwrap();
                       let actual = actual
                           .iter()
                           .map(|opt| opt.scheme_id())
                           .cloned()
                           .collect::<Vec<_>>();
                       assert_eq!(test_case.expected_resolved_auths, actual);
                    }
                    """,
                    *codegenScope,
                )
            }
        }
        project.compileAndTest()
    }
}
