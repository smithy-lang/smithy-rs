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

internal class NestedCollectionCompatibilityTest {
    @Test
    fun `nested XML collections preserve legacy member schemas`() {
        val run = Files.createTempDirectory("nested-collection-parity-")
        val model =
            """
            ${'$'}version: "2"
            namespace test.nestedcollections
            use aws.protocols#restXml
            @restXml service Nested { version: "1", operations: [Inspect] }
            @http(method: "POST", uri: "/inspect")
            operation Inspect {
                input := { maps: OuterMap, lists: Rows, structMaps: OuterStructMap }
                output := { summary: String }
            }
            map OuterMap { key: String, @xmlName("inner") value: InnerMap }
            map InnerMap { @xmlName("itemKey") key: String, @xmlName("itemValue") value: Integer }
            list Rows { @xmlName("row") member: Flags }
            list Flags { @xmlName("flag") member: Boolean }
            map OuterStructMap { key: String, @xmlName("inner") value: InnerStructMap }
            map InnerStructMap { @xmlName("itemKey") key: String, @xmlName("record") value: Record }
            structure Record { text: String }
            """.asSmithyModel()
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
                    tokioTest("nested_xml_collections") {
                        rustTemplate(
                            """
                            let app = crate::Nested::builder(crate::NestedConfig::builder().build())
                                .inspect(|input: crate::input::InspectInput| async move {
                                    crate::output::InspectOutput::builder().summary(#{Some}(format!(
                                        "maps={:?}; lists={:?}; structures={:?}",
                                        input.maps, input.lists, input.struct_maps,
                                    ))).build()
                                }).build().unwrap();
                            let cases = [
                                ("maps", "<InspectInput><maps><entry><key>outer</key><inner><entry><itemKey>inner</itemKey><itemValue>7</itemValue></entry></inner></entry></maps></InspectInput>"),
                                ("lists", "<InspectInput><lists><row><flag>true</flag><flag>false</flag></row></lists></InspectInput>"),
                                ("structures", "<InspectInput><structMaps><entry><key>outer</key><inner><entry><itemKey>inner</itemKey><record><text>ok</text></record></entry></inner></entry></structMaps></InspectInput>"),
                            ];
                            let mut records = #{String}::new();
                            for (name, payload) in cases {
                                let request = #{Http}::Request::post("/inspect")
                                    .header("content-type", "application/xml")
                                    .body(#{Server}::body::to_boxed_sync(payload)).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(app.clone(), request).await.unwrap();
                                let status = response.status().as_u16();
                                let mut headers: #{Vec}<_> = response.headers().iter()
                                    .map(|(key, value)| (key.to_string(), value.to_str().unwrap().to_string())).collect();
                                headers.sort();
                                #{Collect}
                                let record = format!("{name}: {status} {headers:?} {}\n", ::std::str::from_utf8(&body).unwrap());
                                println!("{record}");
                                records.push_str(&record);
                                ${if (!schemaSerde) "assert_eq!(status, 200);" else ""}
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
        val legacy = Files.readString(run.resolve("false.txt"))
        assertEquals(
            """
            maps: 200 [("content-length", "133"), ("content-type", "application/xml")] <InspectOutput><summary>maps=Some({&quot;outer&quot;: {&quot;inner&quot;: 7}}); lists=None; structures=None</summary></InspectOutput>
            lists: 200 [("content-length", "105"), ("content-type", "application/xml")] <InspectOutput><summary>maps=None; lists=Some([[true, false]]); structures=None</summary></InspectOutput>
            structures: 200 [("content-length", "169"), ("content-type", "application/xml")] <InspectOutput><summary>maps=None; lists=None; structures=Some({&quot;outer&quot;: {&quot;inner&quot;: Record { text: Some(&quot;ok&quot;) }}})</summary></InspectOutput>
            """.trimIndent() + "\n",
            legacy,
            run.toString(),
        )
        assertEquals(legacy, Files.readString(run.resolve("true.txt")), run.toString())
    }

    @Test
    fun `sparse and mixed nested JSON collections retain their values`() {
        val run = Files.createTempDirectory("nested-json-parity-")
        val model =
            """
            ${'$'}version: "2"
            namespace test.nestedjson
            use aws.protocols#restJson1
            @restJson1 service Nested { version: "1", operations: [Read] }
            @http(method: "POST", uri: "/read")
            operation Read {
                input := { maps: OuterMap, lists: Rows, deepMaps: DeepMap, mixed: Maps }
                output := { maps: OuterMap, lists: Rows, deepMaps: DeepMap, mixed: Maps }
            }
            map OuterMap { key: String, value: SparseMap }
            @sparse map SparseMap { key: String, value: Boolean }
            list Rows { member: SparseFlags }
            @sparse list SparseFlags { member: Boolean }
            map DeepMap { key: String, value: MiddleMap }
            map MiddleMap { key: String, value: InnerMap }
            map InnerMap { key: String, value: Integer }
            list Maps { member: InnerMap }
            """.asSmithyModel()
        for (schemaSerde in listOf(false, true)) {
            val snapshot = run.resolve("$schemaSerde.txt")
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ObjectNode.parse("""{"codegen":{"http-1x":true,"schemaSerde":$schemaSerde}}""").expectObjectNode(),
                    overrideTestDir = run.resolve(if (schemaSerde) "schema" else "legacy").toFile(),
                    cargoCommand = "cargo test --quiet --lib",
                ),
                testCoverage = HttpTestType.Default,
            ) { context, crate ->
                crate.testModule {
                    tokioTest("sparse_and_mixed_nested_collections") {
                        rustTemplate(
                            """
                            let app = crate::Nested::builder(crate::NestedConfig::builder().build())
                                .read(|input: crate::input::ReadInput| async move {
                                    crate::output::ReadOutput {
                                        maps: input.maps,
                                        lists: input.lists,
                                        deep_maps: input.deep_maps,
                                        mixed: input.mixed,
                                    }
                                }).build().unwrap();
                            let cases = [
                                "{\"maps\":{\"outer\":{\"inner\":true}}}",
                                "{\"maps\":{\"outer\":{\"inner\":null}}}",
                                "{\"lists\":[[true,null,false]]}",
                                "{\"deepMaps\":{\"outer\":{\"middle\":{\"inner\":7}}}}",
                                "{\"mixed\":[{\"inner\":7}]}",
                            ];
                            let mut records = #{String}::new();
                            for payload in cases {
                                let request = #{Http}::Request::post("/read")
                                    .header("content-type", "application/json")
                                    .body(#{Server}::body::to_boxed_sync(payload)).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(app.clone(), request).await.unwrap();
                                let status = response.status().as_u16();
                                let mut headers: #{Vec}<_> = response.headers().iter()
                                    .map(|(key, value)| (key.to_string(), value.to_str().unwrap().to_string())).collect();
                                headers.sort();
                                #{Collect}
                                assert_eq!(status, 200);
                                assert_eq!(body.as_ref(), payload.as_bytes());
                                records.push_str(&format!("{status} {headers:?} {payload}\n"));
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
        assertEquals(Files.readString(run.resolve("false.txt")), Files.readString(run.resolve("true.txt")), run.toString())
    }
}
