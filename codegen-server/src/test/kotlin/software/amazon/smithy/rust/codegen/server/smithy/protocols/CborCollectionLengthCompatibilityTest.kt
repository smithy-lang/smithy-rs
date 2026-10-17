/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

internal class CborCollectionLengthCompatibilityTest {
    @Test
    fun `generated schema collections retain legacy definite lengths`() {
        val model =
            """
            ${'$'}version: "2"
            namespace test.collections
            use smithy.protocols#rpcv2Cbor
            @rpcv2Cbor
            service Collections { version: "1", operations: [Get] }
            operation Get {
                input := {}
                output := { integers: Integers, map: SparseMap, nested: Nested, strings: Strings }
            }
            @sparse
            list Integers { member: Integer }
            @sparse
            map SparseMap { key: String, value: Integer }
            list Nested { member: Numbers }
            list Numbers { member: Integer }
            list Strings { member: String }
            """.asSmithyModel()
        val observations = Files.createTempDirectory("generated-cbor-collection-lengths")
        for (schemaSerde in listOf(false, true)) {
            val label = if (schemaSerde) "schema" else "legacy"
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":$schemaSerde}}""").expectObjectNode(),
                    cargoCommand = "cargo test --quiet --lib --tests",
                ),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                val serializeResponse =
                    if (schemaSerde) {
                        """
                        use #{Server}::schema::ServerProtocol;
                        let protocol = #{Server}::schema::protocol::RpcV2Cbor::default();
                        let response = protocol.serialize_response(crate::output::GetOutput::SCHEMA, &output);
                        """
                    } else {
                        "let response = #{Server}::response::IntoResponse::<#{Marker}>::into_response(output);"
                    }
                crate.testModule {
                    tokioTest("collection_lengths_on_the_generated_wire") {
                        rustTemplate(
                            """
                            use #{BodyUtil}::BodyExt;
                            for count in [0, 1, 23, 24, 255, 256] {
                                let output = crate::output::GetOutput {
                                    integers: #{Some}([#{Some}(7), #{None}].into()),
                                    map: #{Some}([("key".into(), #{None})].into_iter().collect()),
                                    nested: #{Some}(vec![vec![7, 8]]),
                                    strings: #{Some}(vec!["value".into(); count]),
                                };
                                $serializeResponse
                                assert_eq!(response.status().as_u16(), 200);
                                assert_eq!(response.headers()["content-type"], "application/cbor");
                                assert_eq!(response.headers()["smithy-protocol"], "rpc-v2-cbor");
                                let mut headers = response.headers().iter().map(|(name, value)|
                                    (name.as_str().to_owned(), value.as_bytes().to_vec())
                                ).collect::<#{Vec}<_>>();
                                headers.sort();
                                let headers = format!("{headers:?}");
                                let body = response.into_body().collect().await.unwrap().to_bytes();
                                let mut decoder = #{Cbor}::Decoder::new(&body);
                                assert_eq!(decoder.map().unwrap(), #{None});
                                for _ in 0..4 {
                                    match decoder.str().unwrap().as_ref() {
                                        "integers" => {
                                            assert_eq!(decoder.list().unwrap(), #{Some}(2));
                                            assert_eq!(decoder.integer().unwrap(), 7);
                                            decoder.null().unwrap();
                                        }
                                        "map" => {
                                            assert_eq!(decoder.map().unwrap(), #{Some}(1));
                                            assert_eq!(decoder.str().unwrap(), "key");
                                            decoder.null().unwrap();
                                        }
                                        "nested" => {
                                            assert_eq!(decoder.list().unwrap(), #{Some}(1));
                                            assert_eq!(decoder.list().unwrap(), #{Some}(2));
                                            assert_eq!(decoder.integer().unwrap(), 7);
                                            assert_eq!(decoder.integer().unwrap(), 8);
                                        }
                                        "strings" => {
                                            assert_eq!(decoder.list().unwrap(), #{Some}(count as u64));
                                            for _ in 0..count {
                                                assert_eq!(decoder.str().unwrap(), "value");
                                            }
                                        }
                                        name => panic!("unexpected field {name}"),
                                    }
                                }
                                assert_eq!(decoder.datatype().unwrap(), #{Cbor}::data::Type::Break);
                                decoder.skip().unwrap();
                                assert_eq!(decoder.position(), body.len());
                                let root = ::std::path::Path::new("$observations");
                                ::std::fs::write(root.join(format!("$label-{count}.bin")), &body).unwrap();
                                ::std::fs::write(root.join(format!("$label-{count}.headers")), headers).unwrap();
                            }
                            """,
                            *preludeScope,
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Marker" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType().resolve("protocol::rpc_v2_cbor::RpcV2Cbor"),
                            "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                            "Cbor" to RuntimeType.smithyCbor(context.runtimeConfig),
                        )
                    }
                }
            }
        }
        // Both implementations generated the same model and exercised the same
        // inputs over their HTTP response path. Keep legacy bytes explicit.
        for (count in listOf(0, 1, 23, 24, 255, 256)) {
            check(
                Files.readAllBytes(observations.resolve("legacy-$count.bin"))
                    .contentEquals(Files.readAllBytes(observations.resolve("schema-$count.bin"))),
            ) { "Generated schema CBOR bytes differ from legacy for count=$count; observations: $observations" }
            check(
                Files.readString(observations.resolve("legacy-$count.headers")) ==
                    Files.readString(observations.resolve("schema-$count.headers")),
            ) { "Generated schema CBOR headers differ from legacy for count=$count; observations: $observations" }
        }
    }
}
