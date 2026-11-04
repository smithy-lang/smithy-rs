/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.restjson

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

/**
 * Exercises JSON Document payloads through generated legacy and schema services. It distinguishes
 * an absent body from explicit null, checks all document variants and exact response payloads,
 * and compiles the legacy fallible document helper across both HTTP library versions. This
 * protects the generated helper signature as well as Document serialization and deserialization.
 */
internal class ServerJsonDocumentPayloadTest {
    @Test
    fun `legacy document payload null is a value`() = checkDocumentRequest(schemaSerde = false)

    @Test
    fun `schema document payload null matches legacy`() = checkDocumentRequest(schemaSerde = true)

    private fun checkDocumentRequest(schemaSerde: Boolean) {
        val model =
            """
            ${'$'}version: "2"
            namespace test.documentrequest
            use aws.protocols#restJson1
            @restJson1
            service Documents { version: "1", operations: [PostDocument] }
            @http(method: "POST", uri: "/document")
            operation PostDocument {
                input := { @httpPayload body: Document }
                output := { @httpPayload result: String }
            }
            """.asSmithyModel()
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings =
                    ServerAdditionalSettings.builder().schemaSerde(schemaSerde).toObjectNode(),
                cargoCommand = "cargo test --quiet --lib -- --nocapture",
            ),
            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
        ) { context, crate ->
            crate.testModule {
                tokioTest("document_request_null_is_present") {
                    rustTemplate(
                        """
                        for (request_body, expected) in [
                            ("null", #{Some}(#{Document}::Null)),
                            (" \n null \t", #{Some}(#{Document}::Null)),
                            ("", #{None}),
                            ("true", #{Some}(#{Document}::Bool(true))),
                            ("\"value\"", #{Some}(#{Document}::String("value".to_owned()))),
                            ("[null,false]", #{Some}(#{Document}::Array(vec![#{Document}::Null, #{Document}::Bool(false)]))),
                        ] {
                            let expected_text = format!("{expected:?}");
                            let config = crate::service::DocumentsConfig::builder().build();
                            let app = crate::service::Documents::builder(config)
                                .post_document(move |input: crate::input::PostDocumentInput| {
                                    assert_eq!(input.body, expected);
                                    let text = format!("{:?}", input.body);
                                    async move { crate::output::PostDocumentOutput::builder().result(#{Some}(text)).build() }
                                })
                                .build_unchecked();
                            let request = #{Http}::Request::builder().method("POST").uri("/document")
                                .header("content-type", "application/json")
                                .body(#{Server}::body::to_boxed_sync(request_body)).unwrap();
                            let response = #{Tower}::ServiceExt::oneshot(app, request).await.unwrap();
                            let status = response.status().as_u16();
                            let headers = response.headers().clone();
                            #{BodyToBytes:W}
                            println!("schemaSerde=$schemaSerde {request_body:?}: {status} {headers:?} {body:?}");
                            assert_eq!(status, 200);
                            assert_eq!(headers["content-type"], "text/plain");
                            assert_eq!(headers["content-length"], expected_text.len().to_string());
                            assert_eq!(body.as_ref(), expected_text.as_bytes());
                        }
                        """,
                        *preludeScope,
                        "Document" to RuntimeType.document(context.runtimeConfig),
                        "Http" to RuntimeType.http(context.runtimeConfig),
                        "Tower" to ServerCargoDependency.tower(context).toType(),
                        "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                        "BodyToBytes" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "body", "response"),
                        "EmptyBody" to ServerHttpTestHelpers.createBodyFromBytes(context, "\"\""),
                    )
                }
            }
        }
    }

    @Test
    fun `legacy document helper stays fallible and all document variants serialize through both server paths`() {
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
            val settings = ServerAdditionalSettings.builder().withHttp1x(http1x)
            if (schemaSerde != null) {
                settings.schemaSerde(schemaSerde)
            }
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = settings.toObjectNode(),
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
                                "let serialize = crate::protocol_serde::serialize_document;",
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
                                        assert_eq!(serialize(value).unwrap(), expected.as_bytes());
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
                                    .body(#{EmptyBody:W}).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(app, request).await.unwrap();
                                assert_eq!(response.status().as_u16(), 200);
                                #{BodyToBytes:W}
                                assert_eq!(body.as_ref(), expected.as_bytes());
                                """,
                                *scope,
                                "BodyToBytes" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "body", "response"),
                                "EmptyBody" to ServerHttpTestHelpers.createBodyFromBytes(context, "\"\""),
                            )
                        }
                    }
                }
            }
        }
    }
}
