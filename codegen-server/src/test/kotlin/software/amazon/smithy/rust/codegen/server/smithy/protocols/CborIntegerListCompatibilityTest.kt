/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerInstantiator
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files
import java.nio.file.Path

internal class CborIntegerListCompatibilityTest {
    @Test
    fun `integer list dispatch retains generated legacy wire behavior`() {
        val model =
            """
            ${'$'}version: "2"
            namespace test.integerlists
            use smithy.protocols#rpcv2Cbor
            @rpcv2Cbor
            service Lists { version: "1", operations: [Get] }
            operation Get {
                input := {}
                output := {
                    dense: Numbers, sparse: SparseNumbers, nested: Nested,
                    map: NumberMap, choice: Choice, bounded: BoundedNumbers,
                    constrained: ConstrainedNumbers, memberConstrained: MemberNumbers,
                    enums: EnumNumbers
                }
            }
            list Numbers { member: Integer }
            @sparse list SparseNumbers { member: Integer }
            list Nested { member: Numbers }
            map NumberMap { key: String, value: Numbers }
            union Choice { numbers: Numbers }
            @length(min: 0, max: 256) list BoundedNumbers { member: Integer }
            @range(min: -100, max: 100) integer Small
            list ConstrainedNumbers { member: Small }
            list MemberNumbers { @range(min: -100, max: 100) member: Integer }
            intEnum Number {
                ZERO = 0
                ONE = 1
            }
            list EnumNumbers { member: Number }
            """.asSmithyModel()
        val params =
            Node.parse(
                """
                {
                    "dense": [-2147483648, -25, -1, 0, 24, 2147483647],
                    "sparse": [null, -1, 0, null, 24],
                    "nested": [[], [-1, 24]],
                    "map": {"key": [-1, 24]},
                    "bounded": [-1, 24],
                    "constrained": [-1, 24],
                    "memberConstrained": [-1, 24],
                    "enums": [0, 1]
                }
                """,
            )
        val observations =
            System.getenv("CBOR_INTEGER_OBSERVATIONS")?.let { Files.createDirectories(Path.of(it)) }
                ?: Files.createTempDirectory("generated-cbor-integer-lists")
        val fixtures =
            listOf(
                "collections" to params,
                "union" to Node.parse("""{"choice":{"numbers":[-1,24]}}"""),
            )
        for (publicConstrainedTypes in listOf(false, true)) {
            for (schemaSerde in listOf(false, true)) {
                val label = "${if (schemaSerde) "schema" else "legacy"}-$publicConstrainedTypes"
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        additionalSettings =
                            ObjectNode.parse(
                                """{"codegen":{"schemaSerde":$schemaSerde,"publicConstrainedTypes":$publicConstrainedTypes}}""",
                            ).expectObjectNode(),
                        cargoCommand = "cargo test --quiet --lib --tests",
                    ),
                    testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
                ) { context, crate ->
                    val operation = context.model.lookup<OperationShape>("test.integerlists#Get")
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
                        for ((fixture, fixtureParams) in fixtures) {
                            tokioTest("integer_list_wire_observation_$fixture") {
                                if (!schemaSerde && publicConstrainedTypes && fixture == "collections") {
                                    // Fresh mainline's test instantiator fails to convert constrained
                                    // list elements (E0308). Construct valid typed values explicitly
                                    // so the wire comparison still exercises the unchanged serializer.
                                    rustTemplate(
                                        """
                                        let output = crate::output::GetOutput::builder()
                                            .dense(#{Some}(vec![-2147483648, -25, -1, 0, 24, 2147483647]))
                                            .sparse(#{Some}(vec![#{None}, #{Some}(-1), #{Some}(0), #{None}, #{Some}(24)]))
                                            .nested(#{Some}(vec![vec![], vec![-1, 24]]))
                                            .map(#{Some}(std::collections::HashMap::from([("key".into(), vec![-1, 24])])))
                                            .bounded(#{Some}(vec![-1, 24].try_into().unwrap()))
                                            .constrained(#{Some}(vec![(-1i32).try_into().unwrap(), 24i32.try_into().unwrap()]))
                                            .member_constrained(#{Some}(vec![(-1i32).try_into().unwrap(), 24i32.try_into().unwrap()]))
                                            .enums(#{Some}(vec![0, 1]))
                                            .build();
                                        """,
                                        *preludeScope,
                                    )
                                } else {
                                    withBlock("let output = ", ";") {
                                        ServerInstantiator(context).render(this, operation.outputShape(context.model), fixtureParams)
                                    }
                                }
                                rustTemplate(
                                    """
                                    use #{BodyUtil}::BodyExt;
                                    $serializeResponse
                                    let status = response.status().as_u16();
                                    let mut headers = response.headers().iter().map(|(name, value)|
                                        (name.as_str().to_owned(), value.as_bytes().to_vec())
                                    ).collect::<#{Vec}<_>>();
                                    headers.sort();
                                    let metadata = format!("{status} {headers:?}");
                                    let body = response.into_body().collect().await.unwrap().to_bytes();
                                    let root = ::std::path::Path::new("$observations");
                                    ::std::fs::write(root.join("$label-$fixture.bin"), &body).unwrap();
                                    ::std::fs::write(root.join("$label-$fixture.metadata"), metadata).unwrap();
                                    assert_eq!(status, 200);
                                    """,
                                    *preludeScope,
                                    "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                                    "Marker" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType().resolve("protocol::rpc_v2_cbor::RpcV2Cbor"),
                                    "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                                )
                            }
                        }
                    }
                }
            }
            for (suffix in listOf("bin", "metadata")) {
                check(
                    Files.readAllBytes(observations.resolve("legacy-$publicConstrainedTypes-collections.$suffix"))
                        .contentEquals(Files.readAllBytes(observations.resolve("schema-$publicConstrainedTypes-collections.$suffix"))),
                ) { "Generated schema integer lists differ from legacy; observations: $observations" }
            }
            // The established baseline differs only in the union map framing.
            // Keep both observations explicit rather than changing that behavior.
            val unionBaselines =
                listOf(
                    "legacy" to "bf6663686f696365a1676e756d6265727382201818ff",
                    "schema" to "bf6663686f696365bf676e756d6265727382201818ffff",
                )
            for ((side, expected) in unionBaselines) {
                check(
                    Files.readAllBytes(observations.resolve("$side-$publicConstrainedTypes-union.bin"))
                        .contentEquals(expected.chunked(2).map { it.toInt(16).toByte() }.toByteArray()),
                ) { "Generated $side union changed from the observed baseline; observations: $observations" }
            }
        }
    }
}
