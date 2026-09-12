/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

internal class ServerSchemaCborErrorsTest {
    @Test
    fun `error structures carry their discriminator at every nesting level`() {
        val model =
            """
            ${'$'}version: "2"
            namespace test.errors
            use smithy.protocols#rpcv2Cbor
            use smithy.framework#ValidationException
            @rpcv2Cbor
            service Errors { version: "1", operations: [Get] }
            operation Get {
                input := {}
                output := { direct: Failure, list: Failures, map: FailureMap, choice: Choice }
                errors: [Failure, OuterFailure]
            }
            @error("client")
            structure Failure { message: String }
            @error("client")
            structure OuterFailure { direct: Failure, envelope: Envelope }
            structure Envelope { failure: Failure, next: Envelope, children: Envelopes, validation: ValidationException }
            list Envelopes { member: Envelope }
            list Failures { member: Failure }
            map FailureMap { key: String, value: Failure }
            union Choice { failure: Failure }
        """.asSmithyModel()
        val servers =
            serverIntegrationTest(
                model,
                IntegrationTestParams(additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":true}}""").expectObjectNode()),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                crate.testModule {
                    tokioTest("nested_and_top_level_error_encoding") {
                        rustTemplate(
                            """
                        use #{Server}::schema::ServerProtocol;
                        use #{BodyUtil}::BodyExt;
                        let protocol = #{Server}::protocol::rpc_v2_cbor::RpcV2CborProtocol::default();
                        let failure = || crate::error::Failure { message: #{Some}("failed".into()) };
                        let output = crate::output::GetOutput {
                            direct: #{Some}(failure()),
                            list: #{Some}(vec![failure()]),
                            map: #{Some}([("key".into(), failure())].into_iter().collect()),
                            choice: #{Some}(crate::model::Choice::Failure(failure())),
                        };
                        let response = protocol.serialize_response(crate::output::GetOutput::SCHEMA, &output);
                        assert_eq!(response.status().as_u16(), 200);
                        let bytes = response.into_body().collect().await.unwrap().to_bytes();
                        // Read the wire directly: consuming precisely two error entries and then
                        // the map break detects duplicate __type keys as well as wrong ordering.
                        fn end(decoder: &mut #{Cbor}::Decoder<'_>) {
                            assert_eq!(decoder.datatype().unwrap(), #{Cbor}::data::Type::Break);
                            decoder.skip().unwrap();
                        }
                        fn error(decoder: &mut #{Cbor}::Decoder<'_>) {
                            assert_eq!(decoder.map().unwrap(), #{None});
                            assert_eq!(decoder.str().unwrap(), "__type");
                            assert_eq!(decoder.str().unwrap(), "test.errors##Failure");
                            assert_eq!(decoder.str().unwrap(), "message");
                            assert_eq!(decoder.str().unwrap(), "failed");
                            end(decoder);
                        }
                        let mut decoder = #{Cbor}::Decoder::new(&bytes);
                        assert_eq!(decoder.map().unwrap(), #{None});
                        let mut seen = #{Vec}::new();
                        for _ in 0..4 {
                            let name = decoder.str().unwrap().into_owned();
                            match name.as_str() {
                                "direct" => error(&mut decoder),
                                "list" => {
                                    decoder.list().unwrap();
                                    error(&mut decoder);
                                    end(&mut decoder);
                                }
                                "map" | "choice" => {
                                    decoder.map().unwrap();
                                    assert_eq!(decoder.str().unwrap(), if name == "map" { "key" } else { "failure" });
                                    error(&mut decoder);
                                    end(&mut decoder);
                                }
                                _ => panic!("unexpected output member {name}"),
                            }
                            seen.push(name);
                        }
                        end(&mut decoder);
                        assert_eq!(decoder.position(), bytes.len());
                        seen.sort();
                        assert_eq!(seen, ["choice", "direct", "list", "map"]);

                        let response = protocol.serialize_error(&failure());
                        assert_eq!(response.status().as_u16(), 400);
                        assert_eq!(response.headers()["smithy-protocol"], "rpc-v2-cbor");
                        let bytes = response.into_body().collect().await.unwrap().to_bytes();
                        let mut decoder = #{Cbor}::Decoder::new(&bytes);
                        error(&mut decoder);
                        assert_eq!(decoder.position(), bytes.len());

                        let outer = crate::error::OuterFailure {
                            direct: #{Some}(failure()),
                            envelope: #{Some}(crate::model::Envelope { next: #{None}, children: #{None}, validation: #{None}, failure: #{Some}(failure()) }),
                        };
                        let response = protocol.serialize_error(&outer);
                        assert_eq!(response.status().as_u16(), 400);
                        let bytes = response.into_body().collect().await.unwrap().to_bytes();
                        let mut decoder = #{Cbor}::Decoder::new(&bytes);
                        assert_eq!(decoder.map().unwrap(), #{None});
                        assert_eq!(decoder.str().unwrap(), "__type");
                        assert_eq!(decoder.str().unwrap(), "test.errors##OuterFailure");
                        let mut members = #{Vec}::new();
                        for _ in 0..2 {
                            let name = decoder.str().unwrap().into_owned();
                            match name.as_str() {
                                "direct" => error(&mut decoder),
                                "envelope" => {
                                    decoder.map().unwrap();
                                    assert_eq!(decoder.str().unwrap(), "failure");
                                    error(&mut decoder);
                                    end(&mut decoder);
                                }
                                _ => panic!("unexpected error member {name}"),
                            }
                            members.push(name);
                        }
                        end(&mut decoder);
                        assert_eq!(decoder.position(), bytes.len());
                        members.sort();
                        assert_eq!(members, ["direct", "envelope"]);

                        let validation = || crate::error::ValidationException {
                            message: "invalid".into(), field_list: #{None},
                        };
                        fn validation_error(decoder: &mut #{Cbor}::Decoder<'_>) {
                            assert_eq!(decoder.map().unwrap(), #{None});
                            assert_eq!(decoder.str().unwrap(), "__type");
                            assert_eq!(decoder.str().unwrap(), "smithy.framework##ValidationException");
                            assert_eq!(decoder.str().unwrap(), "message");
                            assert_eq!(decoder.str().unwrap(), "invalid");
                            end(decoder);
                        }
                        let response = protocol.serialize_error(&validation());
                        let bytes = response.into_body().collect().await.unwrap().to_bytes();
                        let mut decoder = #{Cbor}::Decoder::new(&bytes);
                        validation_error(&mut decoder);
                        assert_eq!(decoder.position(), bytes.len());

                        let envelope = crate::model::Envelope {
                            failure: #{None}, next: #{None}, children: #{None}, validation: #{Some}(validation()),
                        };
                        let response = protocol.serialize_response(crate::model::Envelope::SCHEMA, &envelope);
                        let bytes = response.into_body().collect().await.unwrap().to_bytes();
                        let mut decoder = #{Cbor}::Decoder::new(&bytes);
                        assert_eq!(decoder.map().unwrap(), #{None});
                        assert_eq!(decoder.str().unwrap(), "validation");
                        validation_error(&mut decoder);
                        end(&mut decoder);
                        assert_eq!(decoder.position(), bytes.len());
                        """,
                            *preludeScope,
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Cbor" to CargoDependency.smithyCbor(context.runtimeConfig).toType(),
                            "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                        )
                    }
                }
            }
        servers.forEach { it.path.resolve("src/protocol_serde").toFile().exists() shouldBe false }
    }
}
