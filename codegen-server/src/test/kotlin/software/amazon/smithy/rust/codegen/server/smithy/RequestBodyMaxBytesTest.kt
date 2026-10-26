/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

internal class RequestBodyMaxBytesTest {
    @Test
    fun `nested body limit is enforced in both serde modes and overrides the legacy setting`() {
        val model =
            """
            ${'$'}version: "2"
            namespace test
            use aws.protocols#restJson1
            @restJson1 service Example { operations: [Echo, Upload] }
            @http(method: "POST", uri: "/echo")
            operation Echo { input := { @required message: String } output := {} }
            @http(method: "POST", uri: "/upload")
            operation Upload { input := { @required @httpPayload data: Blob } output := {} }
            """.asSmithyModel()
        for ((schemaSerde, version) in listOf(
            false to HttpTestVersion.HTTP_0_X,
            false to HttpTestVersion.HTTP_1_X,
            true to HttpTestVersion.HTTP_1_X,
        )) {
            for (limit in listOf(16, 0)) {
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        additionalSettings =
                            Node.parse(
                                """
                            {
                                "codegen": {"schemaSerde": $schemaSerde, "requestBodyMaxBytes": ${if (limit == 0) 16 else 1024}},
                                "customizationConfig": {"protocols": {"common": {"request": {"body": {"maxBytes": $limit}}}}}
                            }
                            """,
                            ).expectObjectNode(),
                        cargoCommand = "cargo test --quiet --lib",
                    ),
                    testCoverage = HttpTestType.Only(version),
                ) { context, crate ->
                    crate.testModule {
                        tokioTest("nested_request_body_limit") {
                            rustTemplate(
                                """
                                use #{Tower}::ServiceExt;
                                let app = crate::Example::builder(crate::ExampleConfig::builder().build())
                                    .echo(|_: crate::input::EchoInput| async { #{Ok}::<_, crate::error::EchoError>(crate::output::EchoOutput {}) })
                                    .upload(|_: crate::input::UploadInput| async { #{Ok}::<_, crate::error::UploadError>(crate::output::UploadOutput {}) })
                                    .build().unwrap();
                                for (uri, content_type, payload, oversized) in [
                                    ("/echo", "application/json", br##"{"message":"ok"}"##.as_slice(), false),
                                    ("/echo", "application/json", br##"{"message":"longer than sixteen bytes"}"##.as_slice(), true),
                                    ("/upload", "application/octet-stream", b"ok".as_slice(), false),
                                    ("/upload", "application/octet-stream", b"longer than sixteen bytes".as_slice(), true),
                                ] {
                                    let body_bytes = payload.to_vec();
                                    let request = #{Http}::Request::builder()
                                        .method("POST").uri(uri).header("Content-Type", content_type)
                                        .body(#{Body}).unwrap();
                                    let response = app.clone().oneshot(request).await.unwrap();
                                    assert_eq!(response.status().as_u16(), if oversized && $limit > 0 { 400 } else { 200 }, "{uri}");
                                }
                                """,
                                "Tower" to ServerCargoDependency.tower(context).toType(),
                                "Http" to RuntimeType.http(context.runtimeConfig),
                                "Body" to ServerHttpTestHelpers.createBodyFromBytes(context, "body_bytes"),
                                *RuntimeType.preludeScope,
                            )
                        }
                    }
                }
            }
        }
    }

    private fun requestBodyMaxBytes(maxBytes: Long): ObjectNode =
        Node.parse(
            """{"codegen": {"requestBodyMaxBytes": $maxBytes}}""",
        ).expectObjectNode()

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
                additionalSettings = requestBodyMaxBytes(1024),
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
                additionalSettings = requestBodyMaxBytes(16),
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

    @Test
    fun `httpPayload request exceeding body size limit is rejected`() {
        serverIntegrationTest(
            httpPayloadModel,
            IntegrationTestParams(
                additionalSettings = requestBodyMaxBytes(16),
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
