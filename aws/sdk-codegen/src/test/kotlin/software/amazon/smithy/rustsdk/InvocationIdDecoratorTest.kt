/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import SdkCodegenIntegrationTest
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest

class InvocationIdDecoratorTest {
    @Test
    fun customInvocationIdGenerator() {
        awsSdkIntegrationTest(SdkCodegenIntegrationTest.model) { context, rustCrate ->
            val rc = context.runtimeConfig
            val moduleName = context.moduleUseName()
            rustCrate.integrationTest("custom_invocation_id") {
                rustTemplate(
                    """
                    ##[allow(deprecated)]
                    ##[#{tokio}::test]
                    async fn custom_invocation_id() {
                        ##[derive(::std::fmt::Debug)]
                        struct TestIdGen;
                        impl #{InvocationIdGenerator} for TestIdGen {
                            fn generate(&self) -> #{Result}<#{Option}<#{InvocationId}>, #{BoxError}> {
                                #{Ok}(#{Some}(#{InvocationId}::new("custom".into())))
                            }
                        }
                        impl #{GenerateInvocationId} for TestIdGen {
                            fn generate(&self) -> #{Result}<#{Option}<#{GenericClientInvocationId}>, #{BoxError}> {
                                #{Ok}(#{Some}(#{GenericClientInvocationId}::new("custom")))
                            }
                        }

                        async fn verify_request_header<F>(builder: $moduleName::config::Builder, config_verifier: F)
                        where
                            F: Fn(&$moduleName::Config) -> bool
                        {
                            let (http_client, rx) = #{capture_request}(#{None});
                            let config = builder.http_client(http_client).build();
                            config_verifier(&config);
                            let client = $moduleName::Client::from_conf(config);
                            let _ = dbg!(client.some_operation().send().await);
                            let request = rx.expect_request();
                            assert_eq!("custom", request.headers().get("amz-sdk-invocation-id").unwrap());
                        }

                        verify_request_header( $moduleName::Config::builder()
                            .invocation_id_generator(TestIdGen), |cfg| cfg.invocation_id_generator().is_some()).await;

                        verify_request_header( $moduleName::Config::builder()
                            .invocation_id_generator_v2(TestIdGen), |cfg| cfg.invocation_id_generator_v2().is_some()).await;
                    }
                    """,
                    *preludeScope,
                    "tokio" to CargoDependency.Tokio.toType(),
                    "GenerateInvocationId" to
                        RuntimeType.smithyRuntime(rc)
                            .resolve("client::invocation_id::GenerateInvocationId"),
                    "GenericClientInvocationId" to
                        RuntimeType.smithyRuntime(rc)
                            .resolve("client::invocation_id::InvocationId"),
                    "HttpRequest" to
                        RuntimeType.smithyRuntimeApi(rc)
                            .resolve("client::orchestrator::HttpRequest"),
                    "InvocationIdGenerator" to
                        AwsRuntimeType.awsRuntime(rc)
                            .resolve("invocation_id::InvocationIdGenerator"),
                    "InvocationId" to
                        AwsRuntimeType.awsRuntime(rc)
                            .resolve("invocation_id::InvocationId"),
                    "BoxError" to RuntimeType.boxError(rc),
                    "capture_request" to RuntimeType.captureRequest(rc),
                )
            }
        }
    }

    @Test
    fun defaultInvocationIdGenerator() {
        awsSdkIntegrationTest(SdkCodegenIntegrationTest.model) { context, rustCrate ->
            val rc = context.runtimeConfig
            val moduleName = context.moduleUseName()
            rustCrate.integrationTest("default_invocation_id") {
                rustTemplate(
                    """
                    ##[allow(deprecated)]
                    ##[#{tokio}::test]
                    async fn default_invocation_id() {
                        let (http_client, rx) = #{capture_request}(None);
                        // not calling `.invocation_id_generator` to trigger the default
                        let config = $moduleName::Config::builder()
                            .http_client(http_client)
                            .build();
                        assert!(config.invocation_id_generator().is_none());
                        assert!(config.invocation_id_generator_v2().is_none());

                        let client = $moduleName::Client::from_conf(config);

                        let _ = dbg!(client.some_operation().send().await);
                        let request = rx.expect_request();
                        // UUID should include 32 chars and 4 dashes
                        assert_eq!(36, request.headers().get("amz-sdk-invocation-id").unwrap().len());
                    }
                    """,
                    *preludeScope,
                    "tokio" to CargoDependency.Tokio.toType(),
                    "InvocationIdGenerator" to
                        AwsRuntimeType.awsRuntime(rc)
                            .resolve("invocation_id::InvocationIdGenerator"),
                    "InvocationId" to
                        AwsRuntimeType.awsRuntime(rc)
                            .resolve("invocation_id::InvocationId"),
                    "BoxError" to RuntimeType.boxError(rc),
                    "capture_request" to RuntimeType.captureRequest(rc),
                )
            }
        }
    }

    @Test
    fun noInvocationIdGenerator() {
        awsSdkIntegrationTest(SdkCodegenIntegrationTest.model) { context, rustCrate ->
            val rc = context.runtimeConfig
            val moduleName = context.moduleUseName()
            rustCrate.integrationTest("no_invocation_id") {
                rustTemplate(
                    """
                    ##[allow(deprecated)]
                    ##[#{tokio}::test]
                    async fn no_invocation_id() {
                        /// A "generator" that always returns `None`.
                        ##[derive(Debug, Default)]
                        struct NoInvocationIdGenerator;
                        impl #{InvocationIdGenerator} for NoInvocationIdGenerator {
                            fn generate(&self) -> #{Result}<#{Option}<#{InvocationId}>, #{BoxError}> {
                                #{Ok}(#{None})
                            }
                        }
                        impl #{GenerateInvocationId} for NoInvocationIdGenerator {
                            fn generate(&self) -> #{Result}<#{Option}<#{GenericClientInvocationId}>, #{BoxError}> {
                                #{Ok}(#{None})
                            }
                        }

                        async fn verify_request_header<F>(builder: $moduleName::config::Builder, config_verifier: F)
                        where
                            F: Fn(&$moduleName::Config) -> bool
                        {
                            let (http_client, rx) = #{capture_request}(#{None});
                            let config = builder.http_client(http_client).build();
                            config_verifier(&config);
                            let client = $moduleName::Client::from_conf(config);
                            let _ = dbg!(client.some_operation().send().await);
                            let request = rx.expect_request();
                            assert!(request.headers().get("amz-sdk-invocation-id").is_none());
                        }

                        verify_request_header( $moduleName::Config::builder()
                            .invocation_id_generator(NoInvocationIdGenerator), |cfg| cfg.invocation_id_generator().is_some()).await;

                        verify_request_header( $moduleName::Config::builder()
                            .invocation_id_generator_v2(NoInvocationIdGenerator), |cfg| cfg.invocation_id_generator_v2().is_some()).await;
                    }
                    """,
                    *preludeScope,
                    "tokio" to CargoDependency.Tokio.toType(),
                    "GenerateInvocationId" to
                        RuntimeType.smithyRuntime(rc)
                            .resolve("client::invocation_id::GenerateInvocationId"),
                    "GenericClientInvocationId" to
                        RuntimeType.smithyRuntime(rc)
                            .resolve("client::invocation_id::InvocationId"),
                    "HttpRequest" to
                        RuntimeType.smithyRuntimeApi(rc)
                            .resolve("client::orchestrator::HttpRequest"),
                    "InvocationIdGenerator" to
                        AwsRuntimeType.awsRuntime(rc)
                            .resolve("invocation_id::InvocationIdGenerator"),
                    "InvocationId" to
                        AwsRuntimeType.awsRuntime(rc)
                            .resolve("invocation_id::InvocationId"),
                    "BoxError" to RuntimeType.boxError(rc),
                    "capture_request" to RuntimeType.captureRequest(rc),
                )
            }
        }
    }
}
