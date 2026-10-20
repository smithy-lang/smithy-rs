/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

/**
 * Send formatted timestamps through generated request deserialization and response serialization.
 * Reading the request correctly is insufficient: using a bare timestamp schema when writing a
 * collection silently replaces its member's format with the protocol default. Pin the response
 * text and compare generated serde and schema services using the same model and requests.
 */
internal class CollectionTimestampFormatCompatibilityTest {
    @Test
    fun `JSON collection timestamps preserve legacy formats`() = checkFormats("restJson1")

    @Test
    fun `XML collection timestamps preserve legacy formats`() = checkFormats("restXml")

    private fun checkFormats(protocol: String) {
        val xml = protocol == "restXml"
        val run = Files.createTempDirectory("collection-timestamp-parity-")
        // Generated serde XML generation cannot compile sparse timestamp maps, so that field is JSON-only.
        val model =
            """
            ${'$'}version: "2"
            namespace test.collectiontimestamps
            use aws.protocols#$protocol
            @$protocol service Times { version: "1", operations: [Echo] }
            @http(method: "POST", uri: "/echo")
            operation Echo { input: EchoInput, output: EchoOutput }
            structure EchoInput {
                dates: HttpDates, mapping: HttpMap, iso: IsoDates, epochs: EpochDates,
                nested: NestedDates, sparse: SparseDates, defaults: DefaultDates,
                nestedMapping: NestedMaps${if (xml) "" else ", sparseMapping: SparseMap"}
            }
            structure EchoOutput {
                dates: HttpDates, mapping: HttpMap, iso: IsoDates, epochs: EpochDates,
                nested: NestedDates, sparse: SparseDates, defaults: DefaultDates,
                nestedMapping: NestedMaps${if (xml) "" else ", sparseMapping: SparseMap"}
            }
            list HttpDates { @timestampFormat("http-date") member: Timestamp }
            map HttpMap { key: String, @timestampFormat("http-date") value: Timestamp }
            list IsoDates { @timestampFormat("date-time") member: Timestamp }
            list EpochDates { @timestampFormat("epoch-seconds") member: Timestamp }
            list NestedDates { member: HttpDates }
            @sparse list SparseDates { @timestampFormat("http-date") member: Timestamp }
            list DefaultDates { member: Timestamp }
            map NestedMaps { key: String, value: HttpMap }
            @sparse map SparseMap { key: String, @timestampFormat("http-date") value: Timestamp }
            """.asSmithyModel()
        val httpDate = "Thu, 01 Jan 1970 00:00:00 GMT"
        val dateTime = "1970-01-01T00:00:00Z"
        // Explicit formats must survive lists, maps, and nesting. Unannotated timestamps retain
        // the protocol default: epoch seconds in JSON and date-time in XML. Sparse JSON cases
        // verify both formatted values and nulls. Single-entry maps keep exact bytes deterministic.
        val cases =
            if (xml) {
                listOf(
                    "<dates><member>$httpDate</member></dates>",
                    "<mapping><entry><key>one</key><value>$httpDate</value></entry></mapping>",
                    "<iso><member>$dateTime</member></iso>",
                    "<epochs><member>0</member></epochs>",
                    "<nested><member><member>$httpDate</member></member></nested>",
                    "<nestedMapping><entry><key>outer</key><value><entry><key>one</key><value>$httpDate</value></entry></value></entry></nestedMapping>",
                    "<defaults><member>$dateTime</member></defaults>",
                ).map { "<EchoInput>$it</EchoInput>" to "<EchoOutput>$it</EchoOutput>" }
            } else {
                listOf(
                    """{"dates":["$httpDate"]}""",
                    """{"mapping":{"one":"$httpDate"}}""",
                    """{"iso":["$dateTime"]}""",
                    """{"epochs":[0]}""",
                    """{"nested":[["$httpDate"]]}""",
                    """{"nestedMapping":{"outer":{"one":"$httpDate"}}}""",
                    """{"sparse":["$httpDate",null]}""",
                    """{"sparseMapping":{"one":"$httpDate"}}""",
                    """{"sparseMapping":{"one":null}}""",
                    """{"defaults":[0]}""",
                ).map { it to it }
            }
        // Only the serde setting changes between generated services; protocol and HTTP version match.
        for (schemaSerde in listOf(false, true)) {
            val snapshot = run.resolve("$schemaSerde.txt")
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ObjectNode.parse("""{"codegen":{"http-1x":true,"schemaSerde":$schemaSerde}}""").expectObjectNode(),
                    overrideTestDir = run.resolve(if (schemaSerde) "schema" else "legacy").toFile(),
                    cargoCommand = "cargo test --quiet --lib -- --nocapture",
                ),
                testCoverage = HttpTestType.Default,
            ) { context, crate ->
                crate.testModule {
                    tokioTest("collection_timestamp_formats") {
                        rustTemplate(
                            """
                            let app = crate::Times::builder(crate::TimesConfig::builder().build())
                                .echo(|input: crate::input::EchoInput| async move {
                                    crate::output::EchoOutput {
                                        dates: input.dates, mapping: input.mapping, iso: input.iso,
                                        epochs: input.epochs, nested: input.nested, sparse: input.sparse,
                                        defaults: input.defaults,
                                        nested_mapping: input.nested_mapping,
                                        ${if (xml) "" else "sparse_mapping: input.sparse_mapping,"}
                                    }
                                }).build().unwrap();
                            let cases = [
                                ${cases.joinToString(",\n") { (request, expected) -> "(${request.dq()}, ${expected.dq()})" }}
                            ];
                            let mut records = #{String}::new();
                            for (payload, expected) in cases {
                                let request = #{Http}::Request::post("/echo")
                                    .header("content-type", ${if (xml) "application/xml".dq() else "application/json".dq()})
                                    .body(#{Server}::body::to_boxed_sync(payload)).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(app.clone(), request).await.unwrap();
                                let status = response.status().as_u16();
                                let mut headers: #{Vec}<_> = response.headers().iter()
                                    .map(|(key, value)| (key.to_string(), value.to_str().unwrap().to_string())).collect();
                                headers.sort();
                                #{Collect}
                                let text = ::std::str::from_utf8(&body).unwrap();
                                println!("{status} {headers:?} {text}");
                                assert_eq!(status, 200);
                                assert_eq!(text, expected);
                                records.push_str(&format!("{status} {headers:?} {text}\n"));
                            }
                            ::std::fs::write(${snapshot.toString().dq()}, records).unwrap();
                            """,
                            *RuntimeType.preludeScope,
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            "Collect" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "body", "response"),
                        )
                    }
                }
            }
        }
        // Compare status and all headers as well as bytes, catching content-length changes too.
        assertEquals(Files.readString(run.resolve("false.txt")), Files.readString(run.resolve("true.txt")), run.toString())
    }
}
