/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.runWithWarnings
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

/**
 * Empty object-valued traits must compile under the same deny-warnings policy as protocol projections.
 * Exercise empty object and array defaults, and verify that fixing Rust emission preserves
 * the trait documents and legacy HTTP responses.
 */
internal class EmptyTraitDocumentCompatibilityTest {
    @Test
    fun `empty trait objects compile with warnings denied and retain their values`() {
        val run = Files.createTempDirectory("empty-trait-document-parity-")
        val model =
            """
            ${'$'}version: "2"
            namespace test.emptytraitdocuments
            use aws.protocols#restJson1
            @restJson1 service Defaults { version: "1", operations: [ReadEmpty, ReadArray] }
            @http(method: "POST", uri: "/empty")
            operation ReadEmpty {
                input := { @default({}) value: Document }
                output := { @default({}) value: Document }
            }
            @http(method: "POST", uri: "/array")
            operation ReadArray {
                input := { @default([]) value: Document }
                output := { @default([]) value: Document }
            }
            """.asSmithyModel()
        for (schemaSerde in listOf(false, true)) {
            val snapshot = run.resolve("$schemaSerde.txt")
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ObjectNode.parse("""{"codegen":{"http-1x":true,"schemaSerde":$schemaSerde}}""").expectObjectNode(),
                    overrideTestDir = run.resolve(if (schemaSerde) "schema" else "legacy").toFile(),
                    // Override inherited RUSTFLAGS so a caller's lint bypass cannot weaken this regression.
                    command = { path -> "cargo test --quiet --lib".runWithWarnings(path) },
                ),
                testCoverage = HttpTestType.Default,
            ) { context, crate ->
                crate.testModule {
                    tokioTest("empty_trait_documents") {
                        rustTemplate(
                            """
                            #{SchemaAssertions}
                            let app = crate::Defaults::builder(crate::DefaultsConfig::builder().build())
                                .read_empty(|input: crate::input::ReadEmptyInput| async move {
                                    crate::output::ReadEmptyOutput { value: input.value }
                                })
                                .read_array(|input: crate::input::ReadArrayInput| async move {
                                    crate::output::ReadArrayOutput { value: input.value }
                                }).build().unwrap();
                            let mut records = #{String}::new();
                            for path in ["/empty", "/array"] {
                                // Explicit input separates the trait-emission check from default application.
                                let request = #{Http}::Request::post(path).header("content-type", "application/json")
                                    .body(#{Server}::body::to_boxed_sync("{\"value\":{}}")).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(app.clone(), request).await.unwrap();
                                let status = response.status().as_u16();
                                let mut headers: #{Vec}<_> = response.headers().iter()
                                    .map(|(key, value)| (key.to_string(), value.to_str().unwrap().to_string())).collect();
                                headers.sort();
                                #{Collect}
                                let text = ::std::str::from_utf8(&body).unwrap();
                                assert_eq!(status, 200);
                                assert_eq!(text, "{\"value\":{}}");
                                records.push_str(&format!("{path} {status} {headers:?} {text}\n"));
                            }
                            ::std::fs::write(${snapshot.toString().dq()}, records).unwrap();
                            """,
                            *RuntimeType.preludeScope,
                            "SchemaAssertions" to
                                writable {
                                    if (schemaSerde) {
                                        rustTemplate(
                                            """
                                        fn default_value<'a>(schema: &'a #{Schema}::Schema<'_>) -> &'a #{Types}::Document {
                                            schema.member_schema("value").unwrap().traits().unwrap()
                                                .get_fqn("smithy.api##default").unwrap()
                                                .as_any().downcast_ref::<#{Schema}::DocumentTrait>().unwrap().value()
                                        }
                                        let empty = #{Types}::Document::Object(#{HashMap}::new());
                                        let array = #{Types}::Document::Array(#{Vec}::new());
                                        assert_eq!(default_value(crate::input::ReadEmptyInput::SCHEMA), &empty);
                                        assert_eq!(default_value(crate::output::ReadEmptyOutput::SCHEMA), &empty);
                                        assert_eq!(default_value(crate::input::ReadArrayInput::SCHEMA), &array);
                                        assert_eq!(default_value(crate::output::ReadArrayOutput::SCHEMA), &array);
                                        """,
                                            *RuntimeType.preludeScope,
                                            "Schema" to RuntimeType.smithySchema(context.runtimeConfig),
                                            "Types" to RuntimeType.smithyTypes(context.runtimeConfig),
                                            "HashMap" to RuntimeType.HashMap,
                                        )
                                    }
                                },
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            "Collect" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "body", "response"),
                        )
                    }
                }
            }
        }
        assertEquals(Files.readString(run.resolve("false.txt")), Files.readString(run.resolve("true.txt")), run.toString())
    }
}
