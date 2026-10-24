/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

internal class RequestBodyMaxBytesTest {
    private val model =
        """
        ${'$'}version: "2.0"
        namespace test

        use aws.protocols#restJson1

        @restJson1
        service TestService {
            operations: [Echo]
        }

        @http(uri: "/echo", method: "POST")
        operation Echo {
            input := {
                @required
                message: String
            }
            output := {
                @required
                message: String
            }
        }
        """.asSmithyModel()

    private val httpPayloadModel =
        """
        ${'$'}version: "2.0"
        namespace test

        use aws.protocols#restJson1

        @restJson1
        service TestService {
            operations: [Upload]
        }

        @http(uri: "/upload", method: "PUT")
        operation Upload {
            input := {
                @required
                @httpPayload
                body: Blob
            }
            output := {
                @required
                size: Long
            }
        }
        """.asSmithyModel()

    @Test
    fun `request within body size limit is accepted`() {
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings =
                    ServerAdditionalSettings.builder()
                        .requestBodyMaxBytes(1024)
                        .toObjectNode(),
            ),
        ) { codegenContext, rustCrate ->
            rustCrate.testModule {
                tokioTest("request_within_limit_succeeds") {
                    rustTemplate(
                        """
                        use crate::input::EchoInput;
                        use #{SmithyHttpServer}::request::FromRequest;

                        let body_bytes = b"{\"message\": \"hello\"}".to_vec();
                        let request = #{Http}::Request::builder()
                            .uri("/echo")
                            .method("POST")
                            .header("Content-Type", "application/json")
                            .body(#{Body})
                            .unwrap();
                        let result = EchoInput::from_request(request).await;
                        result.expect("request within limit should succeed");
                        """,
                        "SmithyHttpServer" to
                            ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType(),
                        "Http" to RuntimeType.http(codegenContext.runtimeConfig),
                        "Body" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "body_bytes"),
                    )
                }
            }
        }
    }

    @Test
    fun `request exceeding body size limit is rejected`() {
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings =
                    ServerAdditionalSettings.builder()
                        .requestBodyMaxBytes(16)
                        .toObjectNode(),
            ),
        ) { codegenContext, rustCrate ->
            rustCrate.testModule {
                tokioTest("request_exceeding_limit_is_rejected") {
                    rustTemplate(
                        """
                        use crate::input::EchoInput;
                        use #{SmithyHttpServer}::request::FromRequest;

                        let body_bytes = b"{\"message\": \"this body is definitely longer than sixteen bytes\"}".to_vec();
                        let request = #{Http}::Request::builder()
                            .uri("/echo")
                            .method("POST")
                            .header("Content-Type", "application/json")
                            .body(#{Body})
                            .unwrap();
                        let result = EchoInput::from_request(request).await;
                        result.expect_err("request exceeding limit should be rejected");
                        """,
                        "SmithyHttpServer" to
                            ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType(),
                        "Http" to RuntimeType.http(codegenContext.runtimeConfig),
                        "Body" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "body_bytes"),
                    )
                }
            }
        }
    }

    private fun settings(
        common: String? = null,
        legacy: String? = null,
    ): ServerRustSettings {
        val codegen = if (legacy != null) ""","codegen": { "requestBodyMaxBytes": $legacy }""" else ""
        val customization =
            if (common != null) {
                ""","customizationConfig": { "protocols": { "common": { "requestBodyMaxBytes": $common } } }"""
            } else {
                ""
            }
        return ServerRustSettings.from(
            model,
            Node.parse(
                """
                {
                    "service": "test#TestService",
                    "module": "test-service",
                    "moduleVersion": "0.0.1",
                    "moduleAuthors": ["test"]
                    $codegen
                    $customization
                }
                """,
            ).expectObjectNode(),
        )
    }

    @Test
    fun `common requestBodyMaxBytes is read and takes precedence over the legacy codegen key`() {
        check(settings().requestBodyMaxBytes() == null)
        check(settings(common = "1024").requestBodyMaxBytes() == 1024L)
        check(settings(legacy = "512").requestBodyMaxBytes() == 512L)
        check(settings(common = "1024", legacy = "512").requestBodyMaxBytes() == 1024L)
        check(settings(common = "0", legacy = "512").requestBodyMaxBytes() == 0L)
    }

    @Test
    fun `invalid common requestBodyMaxBytes values fail generation`() {
        for (invalid in listOf("-1", "1.5", "\"1024\"", "9223372036854775808")) {
            val error = assertThrows<CodegenException> { settings(common = invalid) }
            check(
                error.message!!.contains("customizationConfig.protocols.common.requestBodyMaxBytes"),
            ) { "unexpected message for $invalid: ${error.message}" }
        }
    }

    @Test
    fun `request exceeding common body size limit is rejected`() {
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings =
                    Node.parse(
                        """
                        {
                            "customizationConfig": {
                                "protocols": {
                                    "common": {
                                        "requestBodyMaxBytes": 16
                                    }
                                }
                            }
                        }
                        """,
                    ).expectObjectNode(),
            ),
        ) { codegenContext, rustCrate ->
            rustCrate.testModule {
                tokioTest("request_exceeding_common_limit_is_rejected") {
                    rustTemplate(
                        """
                        use crate::input::EchoInput;
                        use #{SmithyHttpServer}::request::FromRequest;

                        let body_bytes = b"{\"message\": \"this body is definitely longer than sixteen bytes\"}".to_vec();
                        let request = #{Http}::Request::builder()
                            .uri("/echo")
                            .method("POST")
                            .header("Content-Type", "application/json")
                            .body(#{Body})
                            .unwrap();
                        let result = EchoInput::from_request(request).await;
                        result.expect_err("request exceeding limit should be rejected");
                        """,
                        "SmithyHttpServer" to
                            ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType(),
                        "Http" to RuntimeType.http(codegenContext.runtimeConfig),
                        "Body" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "body_bytes"),
                    )
                }
            }
        }
    }

    @Test
    fun `httpPayload request exceeding body size limit is rejected`() {
        serverIntegrationTest(
            httpPayloadModel,
            IntegrationTestParams(
                additionalSettings =
                    ServerAdditionalSettings.builder()
                        .requestBodyMaxBytes(16)
                        .toObjectNode(),
            ),
        ) { codegenContext, rustCrate ->
            rustCrate.testModule {
                tokioTest("http_payload_exceeding_limit_is_rejected") {
                    rustTemplate(
                        """
                        use crate::input::UploadInput;
                        use #{SmithyHttpServer}::request::FromRequest;

                        let body_bytes = vec![0u8; 64];
                        let request = #{Http}::Request::builder()
                            .uri("/upload")
                            .method("PUT")
                            .header("Content-Type", "application/octet-stream")
                            .body(#{Body})
                            .unwrap();
                        let result = UploadInput::from_request(request).await;
                        result.expect_err("httpPayload request exceeding limit should be rejected");
                        """,
                        "SmithyHttpServer" to
                            ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType(),
                        "Http" to RuntimeType.http(codegenContext.runtimeConfig),
                        "Body" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "body_bytes"),
                    )
                }
            }
        }
    }
}
