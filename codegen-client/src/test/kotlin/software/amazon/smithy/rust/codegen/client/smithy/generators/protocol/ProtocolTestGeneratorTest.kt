/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.protocol

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.CommandError
import software.amazon.smithy.rust.codegen.core.util.dq
import java.nio.file.Path
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType as RT

private class TestServiceRuntimePluginCustomization(
    private val context: ClientCodegenContext,
    private val fakeRequestBuilder: String,
    private val fakeRequestBody: String,
) : ServiceRuntimePluginCustomization() {
    override fun section(section: ServiceRuntimePluginSection): Writable =
        writable {
            if (section is ServiceRuntimePluginSection.RegisterRuntimeComponents) {
                val rc = context.runtimeConfig
                section.registerInterceptor(this) {
                    rustTemplate(
                        """
                        {
                            ##[derive(::std::fmt::Debug)]
                            struct TestInterceptor;
                            impl #{Intercept} for TestInterceptor {
                                fn name(&self) -> &'static str {
                                    "TestInterceptor"
                                }

                                fn modify_before_retry_loop(
                                    &self,
                                    context: &mut #{BeforeTransmitInterceptorContextMut}<'_>,
                                    _rc: &#{RuntimeComponents},
                                    _cfg: &mut #{ConfigBag},
                                ) -> #{Result}<(), #{BoxError}> {
                                    // Replace the serialized request
                                    let mut fake_req = #{Http}::Request::builder()
                                        $fakeRequestBuilder
                                        .body(#{SdkBody}::from($fakeRequestBody))
                                        .expect("valid request").try_into().unwrap();
                                    ::std::mem::swap(
                                        context.request_mut(),
                                        &mut fake_req,
                                    );
                                    Ok(())
                                }
                            }

                            TestInterceptor
                        }
                        """,
                        *preludeScope,
                        "BeforeTransmitInterceptorContextMut" to RT.beforeTransmitInterceptorContextMut(rc),
                        "BoxError" to RT.boxError(rc),
                        "ConfigBag" to RT.configBag(rc),
                        "Intercept" to RT.intercept(rc),
                        "RuntimeComponents" to RT.runtimeComponents(rc),
                        "SdkBody" to RT.sdkBody(rc),
                        "Http" to CargoDependency.Http1x.toType(),
                    )
                }
            }
        }
}

private class TestOperationCustomization(
    private val context: ClientCodegenContext,
    private val fakeOutput: String,
) : OperationCustomization() {
    override fun section(section: OperationSection): Writable =
        writable {
            val rc = context.runtimeConfig
            if (section is OperationSection.AdditionalRuntimePluginConfig) {
                rustTemplate(
                    """
                    // Override the default response deserializer with our fake output
                    ##[derive(::std::fmt::Debug)]
                    struct TestDeser;
                    impl #{DeserializeResponse} for TestDeser {
                        fn deserialize_nonstreaming(
                            &self,
                            _response: &#{HttpResponse},
                        ) -> #{Result}<#{Output}, #{OrchestratorError}<#{Error}>> {
                            let fake_out: #{Result}<
                                crate::operation::say_hello::SayHelloOutput,
                                crate::operation::say_hello::SayHelloError,
                            > = $fakeOutput;
                            fake_out
                                .map(|o| #{Output}::erase(o))
                                .map_err(|e| #{OrchestratorError}::operation(#{Error}::erase(e)))
                        }
                    }
                    cfg.store_put(#{SharedResponseDeserializer}::new(TestDeser));
                    """,
                    *preludeScope,
                    "SharedResponseDeserializer" to
                        RT.smithyRuntimeApi(rc)
                            .resolve("client::ser_de::SharedResponseDeserializer"),
                    "Error" to RT.smithyRuntimeApi(rc).resolve("client::interceptors::context::Error"),
                    "HttpResponse" to RT.smithyRuntimeApi(rc).resolve("client::orchestrator::HttpResponse"),
                    "OrchestratorError" to RT.smithyRuntimeApi(rc).resolve("client::orchestrator::OrchestratorError"),
                    "Output" to RT.smithyRuntimeApi(rc).resolve("client::interceptors::context::Output"),
                    "DeserializeResponse" to RT.smithyRuntimeApi(rc).resolve("client::ser_de::DeserializeResponse"),
                )
            }
        }
}

class ProtocolTestGeneratorTest {
    private val model =
        """
        namespace com.example

        use aws.protocols#restJson1
        use smithy.test#httpRequestTests
        use smithy.test#httpResponseTests

        @restJson1
        service HelloService {
            operations: [SayHello],
            version: "1"
        }

        @http(method: "POST", uri: "/")
        @httpRequestTests([
            {
                id: "say_hello",
                protocol: restJson1,
                params: {
                    "greeting": "Hi",
                    "name": "Teddy",
                    "query": "Hello there"
                },
                method: "POST",
                uri: "/",
                queryParams: [
                    "Hi=Hello%20there"
                ],
                forbidQueryParams: [
                    "goodbye"
                ],
                requireQueryParams: ["required"],
                headers: {
                    "X-Greeting": "Hi",
                },
                body: "{\"name\": \"Teddy\"}",
                bodyMediaType: "application/json"
            }
        ])
        @httpResponseTests([{
            id: "basic_response_test",
            protocol: restJson1,
            documentation: "Parses operations with empty JSON bodies",
            body: "{\"value\": \"hey there!\"}",
            params: {"value": "hey there!"},
            bodyMediaType: "application/json",
            headers: {"Content-Type": "application/x-amz-json-1.1"},
            code: 200,
        }])
        operation SayHello {
            input: SayHelloInput,
            output: SayHelloOutput,
            errors: [BadRequest]
        }

        structure SayHelloOutput {
            value: String
        }

        @error("client")
        structure BadRequest {
            message: String
        }

        structure SayHelloInput {
            @httpHeader("X-Greeting")
            greeting: String,

            @httpQuery("Hi")
            query: String,

            name: String
        }
        """.asSmithyModel()
    private val correctBody = """{"name": "Teddy"}"""

    /**
     * Creates a fake HTTP implementation for SayHello & generates the protocol test
     *
     * Returns the [Path] the service was generated at, suitable for running `cargo test`
     */
    private fun testService(
        fakeRequestBuilder: String,
        fakeRequestBody: String = "${correctBody.dq()}.to_string()",
        fakeOutput: String = """Ok(crate::operation::say_hello::SayHelloOutput::builder().value("hey there!").build())""",
    ): Path {
        val codegenDecorator =
            object : ClientCodegenDecorator {
                override val name: String = "mock"
                override val order: Byte = 0

                override fun classpathDiscoverable(): Boolean = false

                override fun serviceRuntimePluginCustomizations(
                    codegenContext: ClientCodegenContext,
                    baseCustomizations: List<ServiceRuntimePluginCustomization>,
                ): List<ServiceRuntimePluginCustomization> =
                    baseCustomizations +
                        TestServiceRuntimePluginCustomization(
                            codegenContext,
                            fakeRequestBuilder,
                            fakeRequestBody,
                        )

                override fun operationCustomizations(
                    codegenContext: ClientCodegenContext,
                    operation: OperationShape,
                    baseCustomizations: List<OperationCustomization>,
                ): List<OperationCustomization> =
                    baseCustomizations + TestOperationCustomization(codegenContext, fakeOutput)
            }
        return clientIntegrationTest(
            model,
            additionalDecorators = listOf(codegenDecorator),
        )
    }

    @Test
    fun `passing e2e protocol request test`() {
        testService(
            """
            .uri("/?Hi=Hello%20there&required")
            .header("X-Greeting", "Hi")
            .method("POST")
            """,
        )
    }

    @Test
    fun `test incorrect response parsing`() {
        val err =
            assertThrows<CommandError> {
                testService(
                    """
                    .uri("/?Hi=Hello%20there&required")
                    .header("X-Greeting", "Hi")
                    .method("POST")
                    """,
                    fakeOutput = "Ok(crate::operation::say_hello::SayHelloOutput::builder().build())",
                )
            }

        err.message shouldContain "basic_response_test_response ... FAILED"
    }

    @Test
    fun `test invalid body`() {
        val err =
            assertThrows<CommandError> {
                testService(
                    """
                    .uri("/?Hi=Hello%20there&required")
                    .header("X-Greeting", "Hi")
                    .method("POST")
                    """,
                    """"{}".to_string()""",
                )
            }

        err.message shouldContain "say_hello_request ... FAILED"
        err.message shouldContain "body did not match"
    }

    @Test
    fun `test invalid url parameter`() {
        val err =
            assertThrows<CommandError> {
                testService(
                    """
                    .uri("/?Hi=INCORRECT&required")
                    .header("X-Greeting", "Hi")
                    .method("POST")
                    """,
                )
            }
        // Verify the test actually ran
        err.message shouldContain "say_hello_request ... FAILED"
        err.message shouldContain "missing query param"
    }

    @Test
    fun `test forbidden url parameter`() {
        val err =
            assertThrows<CommandError> {
                testService(
                    """
                    .uri("/?goodbye&Hi=Hello%20there&required")
                    .header("X-Greeting", "Hi")
                    .method("POST")
                    """,
                )
            }
        // Verify the test actually ran
        err.message shouldContain "say_hello_request ... FAILED"
        err.message shouldContain "forbidden query param"
    }

    @Test
    fun `test required url parameter`() {
        // Hard coded implementation for this 1 test
        val err =
            assertThrows<CommandError> {
                testService(
                    """
                    .uri("/?Hi=Hello%20there")
                    .header("X-Greeting", "Hi")
                    .method("POST")
                    """,
                )
            }

        // Verify the test actually ran
        err.message shouldContain "say_hello_request ... FAILED"
        err.message shouldContain "required query param missing"
    }

    @Test
    fun `test invalid path`() {
        val err =
            assertThrows<CommandError> {
                testService(
                    """
                    .uri("/incorrect-path?required&Hi=Hello%20there")
                    .header("X-Greeting", "Hi")
                    .method("POST")
                    """,
                )
            }

        // Verify the test actually ran
        err.message shouldContain "say_hello_request ... FAILED"
        err.message shouldContain "path was incorrect"
    }

    @Test
    fun `invalid header`() {
        val err =
            assertThrows<CommandError> {
                testService(
                    """
                    .uri("/?Hi=Hello%20there&required")
                    // should be "Hi"
                    .header("X-Greeting", "Hey")
                    .method("POST")
                    """,
                )
            }

        err.message shouldContain "say_hello_request ... FAILED"
        err.message shouldContain "invalid header value"
    }
}
