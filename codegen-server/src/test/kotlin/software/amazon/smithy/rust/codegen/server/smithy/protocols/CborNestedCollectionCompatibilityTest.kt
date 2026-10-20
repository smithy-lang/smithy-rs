/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

internal class CborNestedCollectionCompatibilityTest {
    @Test
    fun `sparse and mixed nested CBOR requests echo legacy values and wire responses`() {
        val run = Files.createTempDirectory("nested-cbor-parity-")
        val model =
            """
            ${'$'}version: "2"
            namespace test.nestedcbor
            use smithy.protocols#rpcv2Cbor
            @rpcv2Cbor service Nested { version: "1", operations: [Read] }
            operation Read {
                input := { maps: OuterMap, lists: Rows, deepMaps: DeepMap, mixed: Maps, mapLists: MapLists }
                output := { maps: OuterMap, lists: Rows, deepMaps: DeepMap, mixed: Maps, mapLists: MapLists }
            }
            map OuterMap { key: String, value: SparseMap }
            @sparse map SparseMap { key: String, value: Boolean }
            list Rows { member: SparseFlags }
            @sparse list SparseFlags { member: Boolean }
            map DeepMap { key: String, value: MiddleMap }
            map MiddleMap { key: String, value: InnerMap }
            map InnerMap { key: String, value: Integer }
            list Maps { member: InnerMap }
            map MapLists { key: String, value: SparseFlags }
            """.asSmithyModel()
        for (schemaSerde in listOf(false, true)) {
            val snapshot = run.resolve("$schemaSerde.txt")
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":$schemaSerde}}""").expectObjectNode(),
                    overrideTestDir = run.resolve(if (schemaSerde) "schema" else "legacy").toFile(),
                    cargoCommand = "cargo test --quiet --lib",
                ),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                check(context.usesSchemaHttpSerde == schemaSerde)
                crate.testModule {
                    tokioTest("sparse_and_mixed_nested_collections") {
                        rustTemplate(
                            """
                            // Encode the same fixture with definite or indefinite containers at every
                            // level, independently of generated code and the schema codec.
                            fn encode(value: &#{Json}::Value, encoder: &mut #{Cbor}::Encoder, indefinite: bool) {
                                match value {
                                    #{Json}::Value::Object(entries) => {
                                        if indefinite { encoder.begin_map(); } else { encoder.map(entries.len()); }
                                        for (key, value) in entries {
                                            encoder.str(key);
                                            encode(value, encoder, indefinite);
                                        }
                                        if indefinite { encoder.end(); }
                                    }
                                    #{Json}::Value::Array(items) => {
                                        if indefinite { encoder.begin_array(); } else { encoder.array(items.len()); }
                                        for value in items { encode(value, encoder, indefinite); }
                                        if indefinite { encoder.end(); }
                                    }
                                    #{Json}::Value::Bool(value) => { encoder.boolean(*value); }
                                    #{Json}::Value::Number(value) => { encoder.long(value.as_i64().unwrap()); }
                                    #{Json}::Value::Null => { encoder.null(); }
                                    _ => panic!("unsupported fixture value"),
                                }
                            }

                            let app = crate::Nested::builder(crate::NestedConfig::builder().build())
                                .read(|input: crate::input::ReadInput| async move {
                                    crate::output::ReadOutput {
                                        maps: input.maps,
                                        lists: input.lists,
                                        deep_maps: input.deep_maps,
                                        mixed: input.mixed,
                                        map_lists: input.map_lists,
                                    }
                                }).build().unwrap();
                            // One key per map keeps byte comparisons independent of HashMap iteration order.
                            let cases = [
                                ("map_of_maps", r##"{"maps":{"outer":{"inner":true}}}"##),
                                ("sparse_inner_map", r##"{"maps":{"outer":{"inner":null}}}"##),
                                ("empty_inner_map", r##"{"maps":{"outer":{}}}"##),
                                ("empty_outer_map", r##"{"maps":{}}"##),
                                ("list_of_lists", r##"{"lists":[[true,false],[false]]}"##),
                                ("sparse_inner_list", r##"{"lists":[[true,null,false],[null]]}"##),
                                ("empty_inner_list", r##"{"lists":[[],[false],[]]}"##),
                                ("empty_outer_list", r##"{"lists":[]}"##),
                                ("deep_maps", r##"{"deepMaps":{"outer":{"middle":{"inner":7}}}}"##),
                                ("empty_deep_map", r##"{"deepMaps":{"outer":{"middle":{}}}}"##),
                                ("list_of_maps", r##"{"mixed":[{"inner":7},{},{"other":-1}]}"##),
                                ("map_of_sparse_lists", r##"{"mapLists":{"outer":[true,null,false]}}"##),
                                ("map_of_empty_lists", r##"{"mapLists":{"outer":[]}}"##),
                            ];
                            let mut records = #{String}::new();
                            for (name, payload) in cases {
                                let expected: #{Json}::Value = #{Json}::from_str(payload).unwrap();
                                for indefinite in [false, true] {
                                    let mut encoder = #{Cbor}::Encoder::new(#{Vec}::new());
                                    encode(&expected, &mut encoder, indefinite);
                                    let request = #{Http}::Request::post("/service/Nested/operation/Read")
                                        .header("smithy-protocol", "rpc-v2-cbor")
                                        .header("content-type", "application/cbor")
                                        .body(#{Server}::body::to_boxed_sync(encoder.into_writer())).unwrap();
                                    let response = #{Tower}::ServiceExt::oneshot(app.clone(), request).await.unwrap();
                                    let status = response.status().as_u16();
                                    assert_eq!(status, 200, "{name}, indefinite={indefinite}");
                                    assert_eq!(response.headers()["content-type"], "application/cbor");
                                    assert_eq!(response.headers()["smithy-protocol"], "rpc-v2-cbor");
                                    let mut headers: #{Vec}<_> = response.headers().iter()
                                        .map(|(key, value)| (key.to_string(), value.to_str().unwrap().to_string())).collect();
                                    headers.sort();
                                    #{Collect}
                                    let mut cursor = ::std::io::Cursor::new(&body);
                                    let actual: #{Json}::Value = #{Ciborium}::de::from_reader(&mut cursor).unwrap();
                                    assert_eq!(cursor.position() as usize, body.len(), "{name}: trailing CBOR data");
                                    assert_eq!(actual, expected, "{name}, indefinite={indefinite}");
                                    let length = headers.iter().find(|(name, _)| name == "content-length").unwrap().1.parse::<usize>().unwrap();
                                    assert_eq!(length, body.len());
                                    records.push_str(&format!("{name} indefinite={indefinite}: {status} {headers:?} {:02x?}\n", body.as_ref()));
                                }
                            }
                            ::std::fs::write(${snapshot.toString().dq()}, records).unwrap();
                            """,
                            *RuntimeType.preludeScope,
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            "Cbor" to RuntimeType.smithyCbor(context.runtimeConfig),
                            "Json" to CargoDependency.SerdeJson.toDevDependency().toType(),
                            "Ciborium" to CargoDependency.Ciborium.toDevDependency().toType(),
                            "Collect" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "body", "response"),
                        )
                    }
                }
            }
        }
        // Keep the full generated serde HTTP observations, including CBOR framing, in the comparison.
        assertEquals(Files.readString(run.resolve("false.txt")), Files.readString(run.resolve("true.txt")), run.toString())
    }
}
