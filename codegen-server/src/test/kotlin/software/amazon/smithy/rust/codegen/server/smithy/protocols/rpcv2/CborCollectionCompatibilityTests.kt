/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.rpcv2

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerInstantiator
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files
import java.nio.file.Path

/**
 * Serializes legacy and schema rpcv2Cbor outputs containing lists, maps, unions, and nested
 * collections. It walks the actual CBOR bytes to check declared collection lengths, values,
 * container endings, and complete consumption of the response. Status and protocol headers are
 * also checked, and the body bytes and headers must match between serde modes.
 */
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
                    additionalSettings = ServerAdditionalSettings.builder().schemaSerde(schemaSerde).toObjectNode(),
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
        // inputs over their HTTP response path. Keep generated serde bytes explicit.
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

/**
 * Serializes integer collections through legacy and schema rpcv2Cbor paths with public constrained
 * types both enabled and disabled. It covers signed boundaries, sparse entries, nested lists,
 * maps, constrained elements, and integer enums, then compares body bytes and response metadata.
 * Union framing has an established legacy/schema difference, so each encoding is pinned separately.
 */
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
                            ServerAdditionalSettings.builder().schemaSerde(schemaSerde).publicConstrainedTypes(publicConstrainedTypes).toObjectNode(),
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

/**
 * Echoes sparse and mixed nested CBOR collections through generated legacy and schema servers.
 * Requests use both definite and indefinite container lengths. It decodes each response, checks
 * all nested values and complete consumption, validates content length and protocol headers,
 * and compares the recorded HTTP metadata and response encoding between the two implementations.
 */
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
                    additionalSettings = ServerAdditionalSettings.builder().schemaSerde(schemaSerde).toObjectNode(),
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

/**
 * Posts a CBOR document containing the values list under its modeled member name to the RPC
 * operation URL. Legacy and schema HTTP 1.x servers must both see two elements and return the
 * same CBOR count response, status, and headers. The test protects RPC document-member encoding
 * for @httpPayload lists and uses RPC routing rather than the operation's modeled REST URI.
 */
internal class RpcV2CborListPayloadCompatibilityTest {
    @Test
    fun `schema serde preserves the generated serde CBOR list member encoding`() {
        val model =
            """
            ${'$'}version: "2"
            namespace test.payloadsupport
            use smithy.protocols#rpcv2Cbor
            @rpcv2Cbor service Payloads { version: "1", operations: [Count] }
            @http(method: "POST", uri: "/count")
            operation Count {
                input := { @required @httpPayload values: Strings }
                output := { @required count: Integer }
            }
            list Strings { member: String }
            """.asSmithyModel()
        val observations = Files.createTempDirectory("cbor-list-payload-")
        // Generate the same valid model with generated serde first, then schema serde.
        for (schemaSerde in listOf(false, true)) {
            val snapshot = observations.resolve("$schemaSerde.txt")
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ServerAdditionalSettings.builder().withHttp1x().schemaSerde(schemaSerde).toObjectNode(),
                    overrideTestDir = observations.resolve("$schemaSerde").toFile(),
                    cargoCommand = "cargo test --quiet --lib",
                ),
                testCoverage = HttpTestType.Default,
            ) { context, crate ->
                crate.testModule {
                    tokioTest("list_payload_uses_its_modeled_member_name") {
                        rustTemplate(
                            """
                            let app = crate::Payloads::builder(crate::PayloadsConfig::builder().build())
                                .count(|input: crate::input::CountInput| async move {
                                    #{Ok}::<_, crate::error::CountError>(crate::output::CountOutput { count: input.values.len() as i32 })
                                }).build().unwrap();
                            let request = #{Http}::Request::post("/service/Payloads/operation/Count")
                                .header("content-type", "application/cbor")
                                .header("smithy-protocol", "rpc-v2-cbor")
                                .body(#{Server}::body::to_boxed_sync(
                                    // A CBOR map with the "values" key and a two-element string array.
                                    vec![0xa1u8, 0x66, b'v', b'a', b'l', b'u', b'e', b's', 0x82, 0x61, b'a', 0x61, b'b']
                                )).unwrap();
                            let response = #{Tower}::ServiceExt::oneshot(app, request).await.unwrap();
                            let status = response.status().as_u16();
                            assert_eq!(status, 200);
                            let mut headers: #{Vec}<_> = response.headers().iter()
                                .map(|(key, value)| (key.to_string(), value.as_bytes().to_vec())).collect();
                            headers.sort();
                            #{Collect}
                            // Preserve generated serde's indefinite-length map encoding for {"count": 2}.
                            assert_eq!(body.as_ref(), &[0xbf, 0x65, b'c', b'o', b'u', b'n', b't', 2, 0xff]);
                            let record = format!("{status} {headers:?} {:?}", body.as_ref());
                            ::std::fs::write(${snapshot.toString().dq()}, record).unwrap();
                            """,
                            *RuntimeType.preludeScope,
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Collect" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "body", "response"),
                        )
                    }
                }
            }
        }
        // Compare the recorded status, sorted headers, and exact CBOR response bytes.
        assertEquals(Files.readString(observations.resolve("false.txt")), Files.readString(observations.resolve("true.txt")))
    }
}
