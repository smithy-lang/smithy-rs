/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.customizations

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

internal class ServerSchemaRpcPayloadTest {
    private fun payloadModel(protocols: String) =
        """
        ${'$'}version: "2"
        namespace test.sharedrpcpayload
        use aws.protocols#awsJson1_0
        use aws.protocols#awsJson1_1
        use aws.protocols#restJson1
        use aws.protocols#restXml
        use smithy.protocols#rpcv2Cbor
        $protocols
        service Payloads { version: "1", operations: [StructPayload, UnionPayload] }
        @http(uri: "/struct", method: "POST", code: 200)
        operation StructPayload {
            input := {}
            output := {
                @httpPayload @xmlName("MemberTarget") body: Target
                @httpHeader("x-h") header: String
            }
            errors: [Failure]
        }
        @http(uri: "/union", method: "POST", code: 200)
        operation UnionPayload {
            input := {}
            output := {
                @httpPayload @xmlName("MemberChoice") body: Choice
                @httpHeader("x-h") header: String
            }
        }
        @xmlName("WireTarget") @xmlNamespace(uri: "urn:target", prefix: "t")
        structure Target { @xmlName("Child") child: String }
        @xmlName("WireChoice") @xmlNamespace(uri: "urn:target", prefix: "t")
        union Choice { @xmlName("Child") child: String }
        @error("client") @httpError(409)
        structure Failure { @httpPayload body: Target }
        """.asSmithyModel()

    @Test
    fun `shared structured payload values retain RPC member keys and REST bindings`() {
        val model = payloadModel("@awsJson1_0 @awsJson1_1 @rpcv2Cbor @restJson1 @restXml")
        // Generate one set of schema types for all five protocols. Keep AWS JSON's
        // default historical response bindings, as in the generated serde comparisons below.
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":true}}""").expectObjectNode(),
                cargoCommand = "cargo test --quiet --lib --tests",
            ),
            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
        ) { context, crate ->
            check(context.usesSchemaHttpSerde)
            crate.testModule {
                tokioTest("shared_structured_payload_responses") {
                    rustTemplate(
                        """
                        use #{Schema}::serde::SerializableStruct;
                        use #{Server}::schema::ServerProtocol;
                        use #{Server}::schema::protocol;

                        fn cbor_document(decoder: &mut #{Cbor}::Decoder<'_>) -> #{Json}::Value {
                            match decoder.datatype().unwrap() {
                                #{Cbor}::data::Type::Map | #{Cbor}::data::Type::MapIndef => {
                                    let length = decoder.map().unwrap();
                                    let mut entries = #{Json}::Map::new();
                                    while length.map(|count| entries.len() < count as usize)
                                        .unwrap_or_else(|| decoder.datatype().unwrap() != #{Cbor}::data::Type::Break) {
                                        let key = decoder.str().unwrap().into_owned();
                                        assert!(entries.insert(key, cbor_document(decoder)).is_none());
                                    }
                                    if length.is_none() { decoder.skip().unwrap(); }
                                    #{Json}::Value::Object(entries)
                                }
                                #{Cbor}::data::Type::String =>
                                    #{Json}::Value::String(decoder.str().unwrap().into_owned()),
                                kind => panic!("unexpected fixture CBOR type: {kind:?}"),
                            }
                        }

                        let aws_json_10 = protocol::AwsJson1_0::default();
                        let aws_json_11 = protocol::AwsJson1_1::default();
                        let cbor = protocol::RpcV2Cbor::default();
                        let rest_json = protocol::RestJson1::default();
                        let rest_xml = protocol::RestXml::default();
                        let protocols: [(&str, &dyn ServerProtocol); 5] = [
                            ("awsJson1_0", &aws_json_10),
                            ("awsJson1_1", &aws_json_11),
                            ("rpcv2Cbor", &cbor),
                            ("restJson1", &rest_json),
                            ("restXml", &rest_xml),
                        ];
                        for present in [true, false] {
                            let structure = crate::output::StructPayloadOutput {
                                body: if present {
                                    #{Some}(crate::model::Target { child: #{Some}("value".into()) })
                                } else { #{None} },
                                header: #{Some}("hv".into()),
                            };
                            let union = crate::output::UnionPayloadOutput {
                                body: if present {
                                    #{Some}(crate::model::Choice::Child("value".into()))
                                } else { #{None} },
                                header: #{Some}("hv".into()),
                            };
                            // Borrow the same generated values for every protocol.
                            let values: [(&str, &dyn SerializableStruct, &str); 2] = [
                                ("structure", &structure, "MemberTarget"),
                                ("union", &union, "MemberChoice"),
                            ];
                            for (name, protocol) in protocols.into_iter().chain(protocols.into_iter().rev()) {
                                for (kind, value, root) in values {
                                    let response = protocol.serialize_response(value.schema(), value);
                                    assert_eq!(response.status(), 200, "{name} {kind} present={present}");
                                    if name != "rpcv2Cbor" {
                                        assert_eq!(response.headers().get("x-h").unwrap(), "hv",
                                            "{name} {kind} present={present}");
                                    }
                                    let length = response.headers().get("content-length").unwrap()
                                        .to_str().unwrap().parse::<usize>().unwrap();
                                    let body = #{BodyUtil}::BodyExt::collect(#{BodyUtil}::Limited::new(response.into_body(), 1024))
                                        .await.unwrap().to_bytes();
                                    assert_eq!(length, body.len(), "{name} {kind} present={present}");
                                    match name {
                                        "restXml" => {
                                            let expected = if present {
                                                format!("<{root} xmlns:t=\"urn:target\"><Child>value</Child></{root}>")
                                            } else { #{String}::new() };
                                            assert_eq!(body.as_ref(), expected.as_bytes(),
                                                "{name} {kind} present={present}");
                                        }
                                        "restJson1" if !present => {
                                            // Generated serde REST JSON emits an empty object for an unset
                                            // structure payload and an empty body for an unset union.
                                            let expected = if kind == "structure" { "{}" } else { "" };
                                            assert_eq!(body.as_ref(), expected.as_bytes(),
                                                "{name} {kind} present={present}");
                                        }
                                        _ => {
                                            let document = if name == "rpcv2Cbor" {
                                                let mut decoder = #{Cbor}::Decoder::new(&body);
                                                let document = cbor_document(&mut decoder);
                                                assert_eq!(decoder.position(), body.len());
                                                document
                                            } else {
                                                #{Json}::from_slice::<#{Json}::Value>(&body).unwrap()
                                            };
                                            let expected = if name == "restJson1" {
                                                #{Json}::json!({"child": "value"})
                                            } else if present {
                                                #{Json}::json!({"body": {"child": "value"}, "header": "hv"})
                                            } else {
                                                #{Json}::json!({"header": "hv"})
                                            };
                                            assert_eq!(document, expected, "{name} {kind} present={present}");
                                        }
                                    }
                                }
                            }
                        }
                        """,
                        "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                        "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                        "Schema" to RuntimeType.smithySchema(context.runtimeConfig),
                        "Cbor" to RuntimeType.smithyCbor(context.runtimeConfig),
                        "Json" to CargoDependency.SerdeJson.toType(),
                        *RuntimeType.preludeScope,
                    )
                }
            }
        }
    }

    @ParameterizedTest
    @ValueSource(strings = ["awsJson1_0", "awsJson1_1", "rpcv2Cbor", "restJson1", "restXml"])
    fun `structured payloads match legacy RPC documents and REST roots`(protocol: String) {
        compareLegacy(protocol)
    }

    @ParameterizedTest
    @ValueSource(strings = ["awsJson1_0", "awsJson1_1"])
    fun `AWS JSON protocol bindings preserve legacy documents with explicit HTTP differences`(protocol: String) {
        compareLegacy(protocol, responseBindings = "smithy-specs")
    }

    private fun compareLegacy(
        protocol: String,
        responseBindings: String? = null,
    ) {
        val marker =
            when (protocol) {
                "awsJson1_0" -> "protocol::aws_json_10::AwsJson1_0"
                "awsJson1_1" -> "protocol::aws_json_11::AwsJson1_1"
                "restJson1" -> "protocol::rest_json_1::RestJson1"
                "restXml" -> "protocol::rest_xml::RestXml"
                else -> "protocol::rpc_v2_cbor::RpcV2Cbor"
            }
        val runtimeProtocol =
            when (protocol) {
                "awsJson1_0" -> "AwsJson1_0"
                "awsJson1_1" -> "AwsJson1_1"
                "restJson1" -> "RestJson1"
                "restXml" -> "RestXml"
                else -> "RpcV2Cbor"
            }
        val model = payloadModel("@$protocol")
        val observations = Files.createTempDirectory("smithy-rpc-payload-$protocol-${responseBindings ?: "historical"}-")
        for (schemaSerde in listOf(false, true)) {
            val snapshot = observations.resolve("$schemaSerde.txt")
            val customization =
                if (responseBindings == null) {
                    ""
                } else {
                    ""","customizationConfig":{"protocols":{"aws.protocols#$protocol":{"responseBindings":"$responseBindings"}}}"""
                }
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":$schemaSerde}$customization}""").expectObjectNode(),
                    cargoCommand = "cargo test --quiet --lib --tests",
                ),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                check(context.usesSchemaHttpSerde == schemaSerde)
                val respond =
                    if (schemaSerde) {
                        "protocol.serialize_response(#{Schema}::serde::SerializableStruct::schema(&output), &output)"
                    } else {
                        "#{Server}::response::IntoResponse::<#{Marker}>::into_response(output)"
                    }
                val respondError =
                    if (schemaSerde) {
                        "protocol.serialize_error(&error)"
                    } else {
                        "#{Server}::response::IntoResponse::<#{Marker}>::into_response(crate::error::StructPayloadError::Failure(error))"
                    }
                val setup =
                    if (schemaSerde && responseBindings != null) {
                        """
                        use #{Server}::schema::{ServerProtocol, MetadataRoutedProtocol, ProtocolBuildContext};
                        let settings = #{Server}::schema::settings::parse_settings_json(
                            br##"{"responseBindings":"$responseBindings"}"##
                        );
                        let context = ProtocolBuildContext::new(&crate::schema::service::PAYLOADS)
                            .with_settings(#{Some}(&settings));
                        let protocol = #{Server}::schema::protocol::$runtimeProtocol::from_build_context(&context).unwrap();
                        """
                    } else if (schemaSerde) {
                        """
                        use #{Server}::schema::ServerProtocol;
                        let protocol = #{Server}::schema::protocol::$runtimeProtocol::default();
                        """
                    } else {
                        ""
                    }
                val errorRecord =
                    """
                    let body = if present {
                        #{Some}(crate::model::Target { child: #{Some}("value".into()) })
                    } else { #{None} };
                    let error = crate::error::Failure { body };
                    records.push_str(&record($respondError).await);
                    """
                crate.testModule {
                    tokioTest("structured_payload_responses") {
                        rustTemplate(
                            """
                            fn cbor_document(decoder: &mut #{Cbor}::Decoder<'_>) -> #{String} {
                                match decoder.datatype().unwrap() {
                                    #{Cbor}::data::Type::Map | #{Cbor}::data::Type::MapIndef => {
                                        let length = decoder.map().unwrap();
                                        let mut entries = #{Vec}::new();
                                        while length.map(|count| entries.len() < count as usize)
                                            .unwrap_or_else(|| decoder.datatype().unwrap() != #{Cbor}::data::Type::Break) {
                                            let key = decoder.str().unwrap();
                                            entries.push(format!("{key:?}:{}", cbor_document(decoder)));
                                        }
                                        if length.is_none() { decoder.skip().unwrap(); }
                                        entries.sort();
                                        format!("{{{}}}", entries.join(","))
                                    }
                                    #{Cbor}::data::Type::String => format!("{:?}", decoder.str().unwrap()),
                                    kind => panic!("unexpected fixture CBOR type: {kind:?}"),
                                }
                            }
                            async fn record(response: #{Server}::response::Response) -> #{String} {
                                let status = response.status().as_u16();
                                let mut headers = response.headers().iter()
                                    .map(|(name, value)| (name.as_str().to_owned(), value.to_str().unwrap().to_owned()))
                                    .collect::<#{Vec}<_>>();
                                headers.sort();
                                let body = #{BodyUtil}::BodyExt::collect(#{BodyUtil}::Limited::new(response.into_body(), 1024))
                                    .await.unwrap().to_bytes();
                                let length = headers.iter().find(|(name, _)| name == "content-length").unwrap();
                                assert_eq!(length.1.parse::<usize>().unwrap(), body.len());
                                // CBOR permits both definite and indefinite maps. Compare their
                                // decoded documents and validate Content-Length separately.
                                let document = if "$protocol" == "rpcv2Cbor" {
                                    let mut decoder = #{Cbor}::Decoder::new(&body);
                                    let document = cbor_document(&mut decoder);
                                    assert_eq!(decoder.position(), body.len());
                                    document
                                } else {
                                    #{String}::from_utf8(body.to_vec()).unwrap()
                                };
                                headers.retain(|(name, _)| name != "content-length");
                                format!("{status}\t{headers:?}\t{document}\n")
                            }
                            $setup
                            let mut records = #{String}::new();
                            for present in [true, false] {
                                let body = if present {
                                    #{Some}(crate::model::Target { child: #{Some}("value".into()) })
                                } else { #{None} };
                                let output = crate::output::StructPayloadOutput { body, header: #{Some}("hv".into()) };
                                records.push_str(&record(
                                    $respond
                                ).await);
                                let body = if present {
                                    #{Some}(crate::model::Choice::Child("value".into()))
                                } else { #{None} };
                                let output = crate::output::UnionPayloadOutput { body, header: #{Some}("hv".into()) };
                                records.push_str(&record(
                                    $respond
                                ).await);
                                $errorRecord
                            }
                            ::std::fs::write("${snapshot.toAbsolutePath()}", records).unwrap();
                            """,
                            "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Marker" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType().resolve(marker),
                            "Schema" to RuntimeType.smithySchema(context.runtimeConfig),
                            "Cbor" to RuntimeType.smithyCbor(context.runtimeConfig),
                            *RuntimeType.preludeScope,
                        )
                    }
                }
            }
        }
        val legacy = Files.readString(observations.resolve("false.txt"))
        val schema = Files.readString(observations.resolve("true.txt"))
        if (responseBindings == null) {
            schema shouldBe legacy
        } else {
            // This setting is intentionally schema-only. Generated serde retains historical
            // headers and @httpError even when given the same customization settings.
            // Preserve both observations and compare their complete documents.
            val legacyRecords = legacy.lineSequence().filter { it.isNotEmpty() }.toList()
            val schemaRecords = schema.lineSequence().filter { it.isNotEmpty() }.toList()
            schemaRecords.size shouldBe legacyRecords.size
            legacyRecords.zip(schemaRecords).forEachIndexed { index, (old, current) ->
                val oldParts = old.split('\t')
                val currentParts = current.split('\t')
                val isError = index % 3 == 2
                oldParts[0] shouldBe if (isError) "409" else "200"
                currentParts[0] shouldBe if (isError) "400" else "200"
                currentParts[1] shouldBe oldParts[1].replace(""", ("x-h", "hv")""", "")
                ObjectNode.parse(currentParts[2]) shouldBe ObjectNode.parse(oldParts[2])
            }
        }
    }
}
