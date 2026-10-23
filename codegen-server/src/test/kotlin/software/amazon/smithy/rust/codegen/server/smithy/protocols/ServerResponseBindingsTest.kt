/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

internal class ServerResponseBindingsTest {
    @ParameterizedTest
    @ValueSource(strings = ["awsJson1_0", "awsJson1_1"])
    fun `AWS JSON defaults to RPC response bindings`(protocol: String) {
        checkResponses(protocol, null)
    }

    @ParameterizedTest
    @ValueSource(strings = ["awsJson1_0", "awsJson1_1"])
    fun `AWS JSON explicitly disables legacy bindings independently`(protocol: String) {
        checkResponses(protocol, false)
    }

    @ParameterizedTest
    @ValueSource(strings = ["awsJson1_0", "awsJson1_1"])
    fun `AWS JSON preserves legacy success and error bindings when enabled`(protocol: String) {
        checkResponses(protocol, true)
    }

    @ParameterizedTest
    @ValueSource(strings = ["restJson1", "restXml", "rpcv2Cbor"])
    fun `REST and CBOR retain their protocol response bindings`(protocol: String) {
        checkResponses(protocol, null)
    }

    private fun checkResponses(
        protocol: String,
        legacy: Boolean?,
    ) {
        val namespace = if (protocol == "rpcv2Cbor") "smithy.protocols" else "aws.protocols"
        val model =
            """
            namespace test
            use $namespace#$protocol
            @$protocol
            service Example { version: "1" operations: [Write] }
            @http(method: "PUT", uri: "/things/{name}", code: 201)
            operation Write {
                input := { @required @httpLabel name: String }
                output: Out
                errors: [ClientFailure, ServerFailure, DefaultClientFailure, DefaultServerFailure]
            }
            structure Out {
                name: String
                @httpHeader("ETag") etag: String
                @httpPrefixHeaders("x-meta-") headers: StringMap
                @httpResponseCode code: Integer
            }
            map StringMap { key: String value: String }
            @error("client")
            @httpError(422)
            structure ClientFailure {
                message: String
                @httpHeader("ETag") etag: String
                @httpPrefixHeaders("x-meta-") headers: StringMap
            }
            @error("server")
            @httpError(503)
            structure ServerFailure {
                message: String
                @httpHeader("ETag") etag: String
                @httpPrefixHeaders("x-meta-") headers: StringMap
            }
            @error("client")
            structure DefaultClientFailure {
                message: String
                @httpHeader("ETag") etag: String
                @httpPrefixHeaders("x-meta-") headers: StringMap
            }
            @error("server")
            structure DefaultServerFailure {
                message: String
                @httpHeader("ETag") etag: String
                @httpPrefixHeaders("x-meta-") headers: StringMap
            }
            """.asSmithyModel(smithyVersion = "2")
        val otherProtocol = if (protocol == "awsJson1_0") "awsJson1_1" else "awsJson1_0"
        val settings =
            if (legacy == null) {
                "{}"
            } else {
                """{"customizationConfig":{"protocols":{"$protocol":{"legacyResponseBindings":$legacy},"$otherProtocol":{"legacyResponseBindings":${!legacy}}}}}"""
            }
        val isRest = protocol == "restJson1" || protocol == "restXml"
        val isRpc = !isRest
        val legacyBindings = legacy == true
        val emitHeaders = isRest || legacyBindings
        val outputStatus = if (emitHeaders) 201 else 200
        val honorErrorStatus = isRest || protocol == "rpcv2Cbor" || legacyBindings
        val clientStatus = if (honorErrorStatus) 422 else 400
        val serverStatus = if (honorErrorStatus) 503 else 500
        val marker =
            when (protocol) {
                "awsJson1_0" -> "protocol::aws_json_10::AwsJson1_0"
                "awsJson1_1" -> "protocol::aws_json_11::AwsJson1_1"
                "restJson1" -> "protocol::rest_json_1::RestJson1"
                "restXml" -> "protocol::rest_xml::RestXml"
                else -> "protocol::rpc_v2_cbor::RpcV2Cbor"
            }
        val contentType =
            when (protocol) {
                "awsJson1_0" -> "application/x-amz-json-1.0"
                "awsJson1_1" -> "application/x-amz-json-1.1"
                "restJson1" -> "application/json"
                "restXml" -> "application/xml"
                else -> "application/cbor"
            }
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = ObjectNode.parse(settings).expectObjectNode(),
                cargoCommand = "cargo test --quiet --lib --tests",
            ),
            testCoverage = HttpTestType.ALL,
        ) { context, crate ->
            val write = context.model.expectShape(ShapeId.from("test#Write"), OperationShape::class.java)
            val json = CargoDependency.SerdeJson.toType()
            val cbor = RuntimeType.smithyCbor(context.runtimeConfig)
            val parseBody =
                if (protocol == "rpcv2Cbor") {
                    """
                    fn decode(decoder: &mut #{Cbor}::decode::Decoder<'_>) -> #{Json}::Value {
                        use #{Cbor}::data::Type;
                        match decoder.datatype().unwrap() {
                            Type::String => #{Json}::Value::String(decoder.str().unwrap().into_owned()),
                            Type::U8 | Type::U16 | Type::U32 | Type::U64 |
                            Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::Int => decoder.long().unwrap().into(),
                            Type::Map | Type::MapIndef => {
                                let length = decoder.map().unwrap();
                                let mut values = #{Json}::Map::new();
                                let mut count = 0;
                                loop {
                                    if let #{Some}(length) = length {
                                        if count == length { break; }
                                    } else if decoder.datatype().unwrap() == Type::Break {
                                        decoder.skip().unwrap();
                                        break;
                                    }
                                    let key = decoder.str().unwrap().into_owned();
                                    values.insert(key, decode(decoder));
                                    count += 1;
                                }
                                #{Json}::Value::Object(values)
                            }
                            other => panic!("unexpected CBOR type: {other:?}"),
                        }
                    }
                    let mut decoder = #{Cbor}::decode::Decoder::new(&body);
                    let document = decode(&mut decoder);
                    assert_eq!(decoder.position(), body.len());
                    """
                } else if (protocol == "restXml") {
                    """
                    let text = #{String}::from_utf8(body.to_vec()).unwrap();
                    assert!(text.contains("<name>n</name>") || text.contains("<message>n</message>"), "{text}");
                    assert!(!text.contains("<etag>") && !text.contains("<headers>") && !text.contains("<code>"), "{text}");
                    let document = #{Json}::json!({});
                    """
                } else {
                    """let document: #{Json}::Value = #{Json}::from_slice(&body).unwrap();"""
                }
            val errorChecks =
                listOf(
                    "ClientFailure" to clientStatus,
                    "ServerFailure" to serverStatus,
                    "DefaultClientFailure" to 400,
                    "DefaultServerFailure" to 500,
                ).joinToString("\n") { (name, status) ->
                    """
                    let error = #{WriteError}::$name(crate::error::$name {
                        message: #{Some}(#{String}::from("n")),
                        etag: #{Some}(#{String}::from("\"abc\"")),
                        headers: #{Some}(headers()),
                    });
                    let response = #{Server}::response::IntoResponse::<#{Marker}>::into_response(error);
                    check_response(response, $status, #{None}, #{Some}("$name")).await;
                    """
                }
            val codes = if (isRest) "[#{None}, #{Some}(299)]" else "[#{None}, #{Some}(299), #{Some}(-1)]"
            val errorType =
                if (protocol == "awsJson1_1") {
                    "name.to_owned()"
                } else {
                    "format!(\"test{}{}\", \"##\", name)"
                }
            crate.testModule {
                tokioTest("protocol_response_bindings") {
                    rustTemplate(
                        """
                        fn headers() -> ::std::collections::HashMap<#{String}, #{String}> {
                            [(#{String}::from("tag"), #{String}::from("v"))].into_iter().collect()
                        }
                        async fn check_response(
                            response: #{Server}::response::Response,
                            expected_status: u16,
                            code: #{Option}<i32>,
                            error: #{Option}<&str>,
                        ) {
                            assert_eq!(response.status().as_u16(), expected_status);
                            assert_eq!(response.headers().get("etag").map(|h| h.to_str().unwrap()), ${if (emitHeaders) """#{Some}("\"abc\"")""" else "#{None}"});
                            assert_eq!(response.headers().get("x-meta-tag").map(|h| h.to_str().unwrap()), ${if (emitHeaders) """#{Some}("v")""" else "#{None}"});
                            assert_eq!(response.headers().get("content-type").unwrap(), "$contentType");
                            ${if (protocol == "rpcv2Cbor") """assert_eq!(response.headers().get("smithy-protocol").unwrap(), "rpc-v2-cbor");""" else ""}
                            let length = response.headers().get("content-length").unwrap().to_str().unwrap().parse::<usize>().unwrap();
                            #{CollectBody}
                            assert_eq!(length, body.len());
                            $parseBody
                            if $isRpc {
                                let mut expected = #{Json}::json!({"etag":"\"abc\"", "headers":{"tag":"v"}});
                                if let #{Some}(name) = error {
                                    expected["message"] = "n".into();
                                    expected["__type"] = $errorType.into();
                                } else {
                                    expected["name"] = "n".into();
                                    if let #{Some}(code) = code { expected["code"] = code.into(); }
                                }
                                assert_eq!(document, expected);
                            } else if "$protocol" == "restJson1" {
                                assert_eq!(document, if error.is_some() { #{Json}::json!({"message":"n"}) } else { #{Json}::json!({"name":"n"}) });
                            }
                        }
                        for code in $codes {
                            let output = #{Output} {
                                name: #{Some}(#{String}::from("n")),
                                etag: #{Some}(#{String}::from("\"abc\"")),
                                headers: #{Some}(headers()),
                                code,
                            };
                            let response = #{Server}::response::IntoResponse::<#{Marker}>::into_response(output);
                            let expected_status = ${if (isRest) "code.unwrap_or(201) as u16" else "$outputStatus"};
                            check_response(response, expected_status, code, #{None}).await;
                        }
                        $errorChecks
                        """,
                        "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                        "Marker" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType().resolve(marker),
                        "Output" to context.symbolProvider.toSymbol(write.outputShape(context.model)),
                        "WriteError" to context.symbolProvider.symbolForOperationError(write),
                        "CollectBody" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "body", "response"),
                        "Json" to json,
                        "Cbor" to cbor,
                        *RuntimeType.preludeScope,
                    )
                }
            }
        }
    }
}
