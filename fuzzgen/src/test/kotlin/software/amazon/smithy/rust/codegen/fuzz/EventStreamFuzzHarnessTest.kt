/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.fuzz

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.build.FileManifest
import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.node.ArrayNode
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.runCommand
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class EventStreamFuzzHarnessTest {
    @ParameterizedTest(name = "event frames: {0}")
    @ValueSource(strings = ["restJson1", "restXml", "awsJson1_0", "awsJson1_1", "rpcv2Cbor"])
    fun `generated harness consumes and produces event frames`(protocol: String) {
        for (multiProtocol in if (protocol.startsWith("awsJson")) listOf(false, true) else listOf(false)) {
            val source = """
                ${'$'}version: "2"
                namespace test.streams
                use ${if (protocol == "rpcv2Cbor") "smithy.protocols" else "aws.protocols"}#$protocol
                @$protocol
                ${if (multiProtocol) "@aws.protocols#restJson1" else ""}
                service Streams { version: "1", operations: [Chat, Upload, Download] }
                @http(method: "POST", uri: "/chat")
                operation Chat { input: StreamingInput, output: StreamingOutput }
                @http(method: "POST", uri: "/upload")
                operation Upload { input: StreamingInput, output: Empty }
                @http(method: "POST", uri: "/download")
                operation Download { input: Empty, output: StreamingOutput }
                structure Empty {}
                structure StreamingInput {
                    @required
                    @httpHeader("x-room")
                    room: String
                    @httpPayload
                    events: Events
                }
                structure StreamingOutput { @httpPayload events: Events }
                @streaming
                union Events { note: Note, bytes: BytesEvent, failure: Failure }
                structure Note { text: String }
                structure BytesEvent {
                    @eventHeader
                    label: String
                    @eventPayload
                    data: Blob
                }
                @error("client")
                structure Failure { message: String }
            """
            for (schema in if (multiProtocol) listOf(true) else listOf(false, true)) {
                val initialRequest = schema && protocol !in listOf("restJson1", "restXml")
                val model =
                    (
                        if (!schema && protocol !in listOf("restJson1", "restXml")) {
                            source.replace(Regex("@required\\s+@httpHeader\\(\"x-room\"\\)\\s+room: String"), "")
                        } else {
                            source
                        }
                    ).asSmithyModel()
                val server =
                    serverIntegrationTest(
                        model,
                        IntegrationTestParams(
                            service = "test.streams#Streams",
                            additionalSettings = Node.objectNode().withMember("codegen", Node.objectNode().withMember("schemaSerde", schema)),
                            command = { dir -> println("generated $protocol schema=$schema at $dir") },
                        ),
                        testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
                    ).single()
                val manifest = FileManifest.create(TestWorkspace.subproject().toPath())
                val runtime = TestRuntimeConfig.runtimeCrateLocation.path!!
                FuzzHarnessBuildPlugin().execute(
                    PluginContext.builder().model(model).fileManifest(manifest).settings(
                        ObjectNode.objectNode()
                            .withMember("service", "test.streams#Streams")
                            .withMember(
                                "targetCrates",
                                ArrayNode.fromNodes(
                                    Node.objectNode().withMember("name", "target").withMember("relativePath", server.path.toString()),
                                ),
                            )
                            .withMember("runtimeConfig", Node.objectNode().withMember("relativePath", runtime)),
                    ).build(),
                )
                val target = manifest.baseDir.resolve("target")
                target.resolve("Cargo.toml").toFile().appendText(
                    """

                    [dev-dependencies]
                    http = "1"
                    aws-smithy-eventstream = { path = "$runtime/aws-smithy-eventstream" }
                    aws-smithy-types = { path = "$runtime/aws-smithy-types" }
                    """.trimIndent(),
                )
                target.resolve("src/lib.rs").toFile().appendText(testCode(protocol, initialRequest))
                "cargo test --quiet".runCommand(target)
            }
        }
    }

    private fun testCode(
        protocol: String,
        initialRequest: Boolean,
    ): String =
        """

        #[cfg(test)]
        mod event_stream_tests {
            use aws_smithy_eventstream::frame::{read_message_from, write_message_to};
            use aws_smithy_types::event_stream::{Message, Header, HeaderValue};
            const PROTOCOL: &str = "$protocol";
            fn frame(kind: &str, name: &str, payload: &[u8]) -> Vec<u8> {
                let content_type = match PROTOCOL {
                    "rpcv2Cbor" => "application/cbor",
                    "restXml" => "application/xml",
                    _ => "application/json",
                };
                let message = Message::new_from_parts(vec![
                    Header::new(":message-type", HeaderValue::String(kind.to_owned().into())),
                    Header::new(if kind == "exception" { ":exception-type" } else { ":event-type" }, HeaderValue::String(name.to_owned().into())),
                    Header::new(":content-type", HeaderValue::String(content_type.into())),
                ], payload.to_vec());
                let mut bytes = Vec::new();
                write_message_to(&message, &mut bytes).unwrap();
                bytes
            }
            fn note() -> Vec<u8> {
                frame("event", "note", match PROTOCOL {
                    "rpcv2Cbor" => b"\xa1\x64text\x65hello",
                    "restXml" => b"<Note><text>hello</text></Note>",
                    _ => br#"{"text":"hello"}"#,
                })
            }
            fn invoke(operation: &str, events: &[u8]) -> aws_smithy_fuzz::FuzzResult {
                let rpc = matches!(PROTOCOL, "awsJson1_0" | "awsJson1_1" | "rpcv2Cbor");
                let streaming = operation != "Download";
                let mut body = Vec::new();
                if $initialRequest && streaming {
                    body.extend(frame("event", "initial-request", if PROTOCOL == "rpcv2Cbor" {
                        b"\xa1\x64room\x64test"
                    } else { br#"{"room":"test"}"# }));
                }
                body.extend_from_slice(events);
                let content_type = if streaming { "application/vnd.amazon.eventstream" } else {
                    match PROTOCOL {
                        "awsJson1_0" => "application/x-amz-json-1.0",
                        "awsJson1_1" => "application/x-amz-json-1.1",
                        "rpcv2Cbor" => "application/cbor",
                        "restXml" => "application/xml",
                        _ => "application/json",
                    }
                };
                if !streaming && rpc { body = if PROTOCOL == "rpcv2Cbor" { vec![0xa0] } else { b"{}".to_vec() }; }
                let uri = match PROTOCOL {
                    "awsJson1_0" | "awsJson1_1" => "/".to_owned(),
                    "rpcv2Cbor" => format!("/service/Streams/operation/{}", operation),
                    _ => format!("/{}", operation.to_lowercase()),
                };
                let mut request = http::Request::builder().method("POST").uri(uri)
                    .header("content-type", content_type).header("x-room", "test");
                if PROTOCOL.starts_with("awsJson") { request = request.header("x-amz-target", format!("Streams.{}", operation)); }
                if PROTOCOL == "rpcv2Cbor" { request = request.header("smithy-protocol", "rpc-v2-cbor"); }
                super::TARGET.lock().unwrap().invoke(request.body(aws_smithy_fuzz::Body::from_bytes(body)).unwrap())
            }
            fn assert_output(bytes: &[u8]) {
                let mut bytes = bytes;
                let mut events = Vec::new();
                while !bytes.is_empty() {
                    let msg = read_message_from(&mut bytes).unwrap();
                    let is_event = msg.headers().iter().any(|h| h.name().as_str() == ":event-type" && matches!(h.value().as_string().unwrap().as_str(), "note" | "bytes"));
                    if is_event { assert!(msg.payload().windows(4).any(|w| w == b"fuzz")); }
                    for h in msg.headers() {
                        if h.name().as_str() == ":event-type" || h.name().as_str() == ":exception-type" {
                            events.push(h.value().as_string().unwrap().as_str().to_owned());
                        }
                    }
                }
                assert!(events.iter().any(|e| e == "note"), "{events:?}");
                assert!(events.iter().any(|e| e == "bytes"), "{events:?}");
                assert!(events.iter().any(|e| e == "failure"), "{events:?}");
            }
            #[test]
            fn input_output_and_duplex_streams() {
                for operation in ["Chat", "Upload", "Download"] {
                    let result = invoke(operation, if operation == "Download" { vec![] } else { [note(), note()].concat() }.as_slice());
                    assert_eq!(result.response.status, 200, "{result:?}");
                    if operation != "Download" { assert!(result.input.unwrap().contains("hello")); }
                    if operation != "Upload" { assert_output(&result.response.body); }
                }
            }
            #[test]
            fn event_headers_and_blob_payloads_are_received() {
                let message = Message::new_from_parts(vec![
                    Header::new(":message-type", HeaderValue::String("event".into())),
                    Header::new(":event-type", HeaderValue::String("bytes".into())),
                    Header::new(":content-type", HeaderValue::String("application/octet-stream".into())),
                    Header::new("label", HeaderValue::String("a-tag".into())),
                ], vec![0, 255, 1]);
                let mut body = Vec::new();
                write_message_to(&message, &mut body).unwrap();
                let result = invoke("Upload", &body);
                let input = result.input.unwrap();
                assert!(input.contains("a-tag"), "{input}");
                assert!(!input.contains("<event-stream-error>"), "{input}");
            }
            #[test]
            fn modeled_exceptions_keep_their_details() {
                let error = frame("exception", "failure", match PROTOCOL {
                    "rpcv2Cbor" => b"\xa1\x67message\x66failed",
                    "restXml" => b"<ErrorResponse><Error><message>failed</message></Error></ErrorResponse>",
                    _ => br#"{"message":"failed"}"#,
                });
                let result = invoke("Upload", &[note(), error].concat());
                let input = result.input.unwrap();
                assert!(input.contains("hello"), "{input}");
                assert!(input.contains("<event-stream-service-error:"), "{input}");
                assert!(input.contains("failed"), "{input}");
            }
            #[test]
            fn empty_and_malformed_streams() {
                let empty = invoke("Upload", &[]);
                assert_eq!(empty.response.status, 200);
                let mut bad = note();
                *bad.last_mut().unwrap() ^= 1;
                let result = invoke("Upload", &[note(), bad].concat());
                let input = result.input.unwrap();
                assert!(input.contains("hello"));
                assert!(input.contains("<event-stream-error>"));
            }
        }
        """.trimIndent()
}
