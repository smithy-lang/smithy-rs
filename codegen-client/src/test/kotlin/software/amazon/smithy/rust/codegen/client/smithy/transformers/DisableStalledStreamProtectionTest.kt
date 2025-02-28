/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.transformers

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.smithy.traits.IncompatibleWithStalledStreamProtectionTrait
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest

class DisableStalledStreamProtectionTest {
    @Test
    fun `it should make event stream operation incompatible with stalled stream protection`() {
        val model =
            DisableStalledStreamProtection.transformModel(
                """
                namespace test

                structure Something { stuff: Blob }

                @streaming
                union SomeStream {
                    Something: Something,
                }

                structure TestInput { inputStream: SomeStream }
                operation TestOperation {
                    input: TestInput,
                }
                service TestService { version: "123", operations: [TestOperation] }
                """.asSmithyModel(smithyVersion = "2.0"),
            )

        val operation = model.expectShape(ShapeId.from("test#TestOperation"))
        assert(operation.hasTrait(IncompatibleWithStalledStreamProtectionTrait.ID))
    }

    @Test
    fun `it should make target operation incompatible with stalled stream protection`() {
        val model =
            """
            namespace test

            structure Something { stuff: Blob }

            structure TestInput { input: Something }
            operation TestOperation {
                input: TestInput,
            }
            service TestService { version: "123", operations: [TestOperation] }
            """.asSmithyModel(smithyVersion = "2.0")

        val transformedModel = (DisableStalledStreamProtection::transformModel)(model)
        // operation should not have IncompatibleWithStalledStreamProtectionTrait,
        // as it is not an event stream
        val operation = transformedModel.expectShape(ShapeId.from("test#TestOperation")) as OperationShape
        assert(!operation.hasTrait(IncompatibleWithStalledStreamProtectionTrait.ID))

        // transformed, however, should have IncompatibleWithStalledStreamProtectionTrait
        val transformed = (DisableStalledStreamProtection::transformOperation)(operation)
        assert(transformed.hasTrait(IncompatibleWithStalledStreamProtectionTrait.ID))
    }

    @Test
    fun `IncompatibleWithStalledStreamProtectionTrait should not add the relevant interceptor`() {
        val model =
            """
            namespace test
            use aws.protocols#restJson1

            structure Something { stuff: Blob }

            @streaming
            union SomeStream {
                Something: Something,
            }

            structure TestOutput { @httpPayload outputStream: SomeStream }

            @http(method: "POST", uri: "/test")
            operation TestOperation {
                output: TestOutput,
            }
            @restJson1
            service TestService { version: "123", operations: [TestOperation] }
            """.asSmithyModel(smithyVersion = "2.0")

        clientIntegrationTest(model) { ctx, crate ->
            crate.integrationTest("disable_stalled_stream_protection") {
                tokioTest("stalled_stream_protection_interceptor_should_be_absent_for_operation") {
                    val moduleName = ctx.moduleUseName()
                    rustTemplate(
                        """
                        use aws_smithy_runtime_api::box_error::BoxError;
                        use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
                        use aws_smithy_types::body::SdkBody;
                        use std::sync::atomic::{AtomicU32, Ordering};
                        use std::sync::Arc;
                        use $moduleName::{
                            config::interceptors::BeforeSerializationInterceptorContextRef,
                            config::Intercept,
                            {Client, Config},
                        };

                        ##[derive(Clone, Debug, Default)]
                        struct TestInterceptor {
                            called: Arc<AtomicU32>,
                        }
                        impl Intercept for TestInterceptor {
                            fn name(&self) -> &'static str {
                                "TestInterceptor"
                            }
                            fn read_before_serialization(
                                &self,
                                _context: &BeforeSerializationInterceptorContextRef<'_>,
                                runtime_components: &RuntimeComponents,
                                _cfg: &mut aws_smithy_types::config_bag::ConfigBag,
                            ) -> Result<(), BoxError> {
                                self.called.fetch_add(1, Ordering::Relaxed);
                                assert!(!runtime_components
                                    .interceptors()
                                    .any(|i| i.name() == "StalledStreamProtectionInterceptor"));
                                Ok(())
                            }
                        }

                        let http_client =
                            #{infallible_client_fn}(|_req| http::Response::builder().body(SdkBody::empty()).unwrap());
                        let test_interceptor = TestInterceptor::default();
                        let client_config = Config::builder()
                            .interceptor(test_interceptor.clone())
                            .endpoint_url("http://localhost:1234")
                            .http_client(http_client)
                            .build();

                        let client = Client::from_conf(client_config);
                        let _ = client.test_operation().send().await;

                        assert!(
                            test_interceptor.called.load(Ordering::Relaxed) == 1,
                            "the interceptor should have been called"
                        );
                        """,
                        "infallible_client_fn" to
                            CargoDependency.smithyRuntimeTestUtil(ctx.runtimeConfig)
                                .toType().resolve("client::http::test_util::infallible_client_fn"),
                    )
                }
            }
        }
    }
}
