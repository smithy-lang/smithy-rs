/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

internal class EventStreamAcceptHeaderTest {
    private val model =
        """
        ${'$'}version: "2.0"
        namespace test

        use smithy.protocols#rpcv2Cbor
        use smithy.framework#ValidationException

        @rpcv2Cbor
        service TestService {
            operations: [StreamingOutputOperation]
        }

        operation StreamingOutputOperation {
            input: StreamingOutputOperationInput
            output: StreamingOutputOperationOutput
            errors: [ValidationException]
        }

        structure StreamingOutputOperationInput {
            message: String
        }

        structure StreamingOutputOperationOutput {
            events: Events
        }

        @streaming
        union Events {
            event: StreamingEvent
        }

        structure StreamingEvent {
            data: String
        }
        """.asSmithyModel()

    @Test
    fun acceptHeaderTests() {
        serverIntegrationTest(model) { codegenContext, rustCrate ->
            rustCrate.testModule {
                generateAcceptHeaderTest(
                    acceptHeader = "application/vnd.amazon.eventstream",
                    shouldFail = false,
                    codegenContext = codegenContext,
                )
                generateAcceptHeaderTest(
                    acceptHeader = "application/cbor",
                    shouldFail = false,
                    codegenContext = codegenContext,
                )
                generateAcceptHeaderTest(
                    acceptHeader = "application/invalid",
                    shouldFail = true,
                    codegenContext = codegenContext,
                )
                generateAcceptHeaderTest(
                    acceptHeader = "application/json, application/cbor",
                    shouldFail = false,
                    codegenContext = codegenContext,
                    testName = "combined_header",
                )
            }
        }
    }

    private fun RustWriter.generateAcceptHeaderTest(
        acceptHeader: String,
        shouldFail: Boolean,
        codegenContext: ServerCodegenContext,
        testName: String = acceptHeader.toSnakeCase(),
    ) {
        val smithyHttpServer = ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType()
        val httpModule = RuntimeType.http(codegenContext.runtimeConfig)
        tokioTest("test_header_$testName") {
            rustTemplate(
                """
                use #{SmithyHttpServer}::request::FromRequest;
                let cbor_empty_bytes = #{Bytes}::copy_from_slice(&#{decode_body_data}(
                    "oA==".as_bytes(),
                    #{MediaType}::from("application/cbor"),
                ));

                let http_request = #{Http}::Request::builder()
                    .uri("/service/TestService/operation/StreamingOutputOperation")
                    .method("POST")
                    .header("Accept", ${acceptHeader.dq()})
                    .header("Content-Type", "application/cbor")
                    .header("smithy-protocol", "rpc-v2-cbor")
                    .body(#{BodyFromCborBytes})
                .unwrap();
                let parsed = crate::input::StreamingOutputOperationInput::from_request(http_request).await;
                """,
                "SmithyHttpServer" to smithyHttpServer,
                "Http" to httpModule,
                "Bytes" to RuntimeType.Bytes,
                "MediaType" to RuntimeType.protocolTest(codegenContext.runtimeConfig, "MediaType"),
                "decode_body_data" to
                    RuntimeType.protocolTest(
                        codegenContext.runtimeConfig,
                        "decode_body_data",
                    ),
                "BodyFromCborBytes" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "cbor_empty_bytes"),
            )

            if (shouldFail) {
                rust("""parsed.expect_err("header should be rejected");""")
            } else {
                rust("""parsed.expect("header should be accepted");""")
            }
        }
    }
}
