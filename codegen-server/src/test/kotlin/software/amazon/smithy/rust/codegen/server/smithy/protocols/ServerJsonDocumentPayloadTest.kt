/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

internal class ServerJsonDocumentPayloadTest {
    @Test
    fun `legacy document helper stays infallible and all document variants serialize through both server paths`() {
        val model =
            """
            ${'$'}version: "2"
            namespace test.document
            use aws.protocols#restJson1
            @restJson1
            service Documents { version: "1", operations: [GetDocument] }
            @http(method: "GET", uri: "/document")
            operation GetDocument {
                input := {}
                output := { @httpPayload body: Document }
            }
            """.asSmithyModel()

        for ((http1x, schemaSerde) in listOf(false to null, false to false, true to null, true to false, true to true)) {
            var codegen = Node.objectNode().withMember("http-1x", http1x)
            if (schemaSerde != null) {
                codegen = codegen.withMember("schemaSerde", schemaSerde)
            }
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = Node.objectNode().withMember("codegen", codegen),
                    cargoCommand = "cargo test --quiet --lib --tests",
                ),
                testCoverage = HttpTestType.Default,
            ) { context, crate ->
                val scope =
                    arrayOf(
                        *preludeScope,
                        "Document" to RuntimeType.document(context.runtimeConfig),
                        "Number" to RuntimeType.smithyTypes(context.runtimeConfig).resolve("Number"),
                        "HashMap" to RuntimeType.HashMap,
                        "Http" to RuntimeType.http(context.runtimeConfig),
                        "Tower" to ServerCargoDependency.tower(context).toType(),
                        "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                    )
                crate.testModule {
                    tokioTest("document_payload_bytes_and_legacy_helper_signature") {
                        if (!context.usesSchemaHttpSerde) {
                            rustTemplate(
                                "let serialize: fn(&#{Document}) -> #{Vec}<u8> = crate::protocol_serde::serialize_document;",
                                *scope,
                            )
                        }
                        rustTemplate(
                            """
                            let cases = [
                                (#{Some}(#{Document}::Null), "null"),
                                (#{Some}(#{Document}::Bool(true)), "true"),
                                (#{Some}(#{Document}::Number(#{Number}::NegInt(-7))), "-7"),
                                (#{Some}(#{Document}::String(#{String}::from("héllo"))), "\"héllo\""),
                                (#{Some}(#{Document}::Array(vec![#{Document}::Null, #{Document}::Bool(false)])), "[null,false]"),
                                (#{Some}(#{Document}::Object(#{HashMap}::from([
                                    (#{String}::from("value"), #{Document}::Bool(true)),
                                ]))), "{\"value\":true}"),
                                (#{None}, ""),
                            ];
                            """,
                            *scope,
                        )
                        rustBlock("for (document, expected) in cases") {
                            if (!context.usesSchemaHttpSerde) {
                                rustTemplate(
                                    """
                                    if let #{Some}(value) = document.as_ref() {
                                        assert_eq!(serialize(value), expected.as_bytes());
                                    }
                                    """,
                                    *scope,
                                )
                            }
                            rustTemplate(
                                """
                                let output = crate::output::GetDocumentOutput { body: document };
                                let config = crate::DocumentsConfig::builder().build();
                                let app = crate::Documents::builder(config)
                                    .get_document(move |_input: crate::input::GetDocumentInput| {
                                        let output = output.clone();
                                        async move { output }
                                    })
                                    .build().expect("service should build");
                                let request = #{Http}::Request::builder()
                                    .method("GET")
                                    .uri("/document")
                                    .header("Content-Type", "application/json")
                                    .body(#{Server}::body::Body::empty()).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(app, request).await.unwrap();
                                assert_eq!(response.status().as_u16(), 200);
                                #{BodyToBytes:W}
                                assert_eq!(body.as_ref(), expected.as_bytes());
                                """,
                                *scope,
                                "BodyToBytes" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "body", "response"),
                            )
                        }
                    }
                }
            }
        }
    }
}
