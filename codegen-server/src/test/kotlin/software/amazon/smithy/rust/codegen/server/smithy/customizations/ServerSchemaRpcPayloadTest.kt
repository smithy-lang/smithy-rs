/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.customizations

import io.kotest.matchers.shouldBe
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.model.node.ObjectNode
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
    @ParameterizedTest
    @ValueSource(strings = ["awsJson1_0", "awsJson1_1", "rpcv2Cbor", "restJson1", "restXml"])
    fun `structured payloads match legacy RPC documents and REST roots`(protocol: String) {
        val namespace = if (protocol == "rpcv2Cbor") "smithy.protocols" else "aws.protocols"
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
                "awsJson1_0" -> "AwsJson1_0Protocol"
                "awsJson1_1" -> "AwsJson1_1Protocol"
                "restJson1" -> "RestJson1Protocol"
                "restXml" -> "RestXmlProtocol"
                else -> "RpcV2CborProtocol"
            }
        val model =
            """
            ${'$'}version: "2"
            namespace test.rpcpayload
            use $namespace#$protocol
            @$protocol
            service Payloads { version: "1", operations: [StructPayload, UnionPayload] }
            @http(uri: "/struct", method: "POST", code: 200)
            operation StructPayload { input := {}, output := { @httpPayload body: Target }, errors: [Failure] }
            @http(uri: "/union", method: "POST", code: 200)
            operation UnionPayload { input := {}, output := { @httpPayload body: Choice } }
            @xmlName("WireTarget")
            structure Target { child: String }
            @xmlName("WireChoice")
            union Choice { child: String }
            @error("client") @httpError(409)
            structure Failure { @httpPayload body: Target }
            """.asSmithyModel()
        val observations = Files.createTempDirectory("smithy-rpc-payload-$protocol-")
        for (schemaSerde in listOf(false, true)) {
            val snapshot = observations.resolve("$schemaSerde.txt")
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":$schemaSerde}}""").expectObjectNode(),
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
                    if (schemaSerde) {
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
                                let body = #{Server}::body::collect_body_limited(response.into_body(), 1024)
                                    .await.unwrap();
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
                                let output = crate::output::StructPayloadOutput { body };
                                records.push_str(&record(
                                    $respond
                                ).await);
                                let body = if present {
                                    #{Some}(crate::model::Choice::Child("value".into()))
                                } else { #{None} };
                                let output = crate::output::UnionPayloadOutput { body };
                                records.push_str(&record(
                                    $respond
                                ).await);
                                $errorRecord
                            }
                            ::std::fs::write("${snapshot.toAbsolutePath()}", records).unwrap();
                            """,
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
        Files.readString(observations.resolve("true.txt")) shouldBe Files.readString(observations.resolve("false.txt"))
    }
}
