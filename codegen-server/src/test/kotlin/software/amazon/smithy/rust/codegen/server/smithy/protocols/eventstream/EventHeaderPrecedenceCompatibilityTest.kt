/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols.eventstream

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class EventHeaderPrecedenceCompatibilityTest {
    @Test
    fun `implicit payload overwrites headers and validates body copies in both modes`() {
        for ((protocol, runtimeProtocol, payload, invalid) in listOf(
            listOf("restJson1", "RestJson1", """br##"{"from":"bob","flag":true,"text":"body"}"##""", """br##"{"flag":"invalid"}"##"""),
            listOf("awsJson1_0", "AwsJson1_0", """br##"{"from":"bob","flag":true,"text":"body"}"##""", """br##"{"flag":"invalid"}"##"""),
            listOf("awsJson1_1", "AwsJson1_1", """br##"{"from":"bob","flag":true,"text":"body"}"##""", """br##"{"flag":"invalid"}"##"""),
            listOf("rpcv2Cbor", "RpcV2Cbor", """b"\xa3\x64from\x63bob\x64flag\xf5\x64text\x64body" """, """b"\xa1\x64flag\x67invalid" """),
        )) {
            val namespace = if (protocol == "rpcv2Cbor") "smithy.protocols" else "aws.protocols"
            val model =
                """
                namespace test
                use $namespace#$protocol
                @$protocol service Example { operations: [Read] }
                @http(method: "POST", uri: "/read")
                operation Read { input := { @httpPayload events: Events } output := {} }
                @streaming union Events { note: Note }
                structure Note { @eventHeader from: String, @eventHeader flag: Boolean, text: String }
                """.asSmithyModel(smithyVersion = "2")
            for (schemaSerde in listOf(false, true)) {
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        additionalSettings = ServerAdditionalSettings.builder().schemaSerde(schemaSerde).toObjectNode(),
                        cargoCommand = "cargo test --quiet --lib",
                    ),
                    testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
                ) { context, crate ->
                    crate.testModule {
                        tokioTest("body_header_precedence") {
                            val constructor =
                                if (schemaSerde) {
                                    "#{Server}::schema::SharedServerProtocol::metadata_routed(#{Server}::schema::protocol::$runtimeProtocol::default())"
                                } else {
                                    ""
                                }
                            rustTemplate(
                                """
                                use #{EventStream}::frame::{UnmarshallMessage, UnmarshalledMessage};
                                use #{Types}::event_stream::{Message, Header, HeaderValue};
                                let unmarshaller = crate::event_stream_serde::EventsUnmarshaller::new($constructor);
                                let headers = vec![
                                    Header::new(":message-type", HeaderValue::String("event".into())),
                                    Header::new(":event-type", HeaderValue::String("note".into())),
                                    Header::new("from", HeaderValue::String("ann".into())),
                                    Header::new("flag", HeaderValue::Bool(false)),
                                ];
                                let message = Message::new_from_parts(headers.clone(), #{Bytes}::copy_from_slice($payload));
                                match unmarshaller.unmarshall(&message).unwrap() {
                                    UnmarshalledMessage::Event(crate::model::Events::Note(note)) => {
                                        assert_eq!(note.from.as_deref(), #{Some}("bob"));
                                        assert_eq!(note.flag, #{Some}(true));
                                        assert_eq!(note.text.as_deref(), #{Some}("body"));
                                    }
                                    other => panic!("unexpected event: {other:?}"),
                                }
                                let message = Message::new_from_parts(headers, #{Bytes}::copy_from_slice($invalid));
                                assert!(unmarshaller.unmarshall(&message).is_err());
                                """,
                                "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                                "EventStream" to CargoDependency.smithyEventStream(context.runtimeConfig).toType(),
                                "Types" to RuntimeType.smithyTypes(context.runtimeConfig),
                                "Bytes" to RuntimeType.Bytes,
                                *RuntimeType.preludeScope,
                            )
                        }
                    }
                }
            }
        }
    }
}
