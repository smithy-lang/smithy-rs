/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols.eventstream

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

/**
 * Sends an ordinary event followed by a modeled exception or malformed frame into generated
 * servers for all five supported protocols. It checks what the handler's receiver observes and
 * records response status, headers, body, and event frames. Schema results must match the legacy
 * captures so stream failures are propagated after successful events with the expected framing.
 */
internal class EventStreamFailureCompatibilityTest {
    @Test
    fun `REST JSON streaming failures match legacy`() = compare("restJson1")

    @Test
    fun `REST XML streaming failures match legacy`() = compare("restXml")

    @Test
    fun `AWS JSON 10 streaming failures match legacy`() = compare("awsJson1_0")

    @Test
    fun `AWS JSON 11 streaming failures match legacy`() = compare("awsJson1_1")

    @Test
    fun `CBOR streaming failures match legacy`() = compare("rpcv2Cbor")

    private fun compare(protocol: String) {
        val run = Files.createTempDirectory("event-stream-failure-$protocol-")
        val cbor = protocol == "rpcv2Cbor"
        val xml = protocol == "restXml"
        val rpc = protocol.startsWith("awsJson") || cbor
        val namespace =
            if (cbor) {
                "smithy.protocols"
            } else {
                "aws.protocols"
            }
        val mediaType =
            when (protocol) {
                "rpcv2Cbor" -> "application/cbor"
                "restXml" -> "application/xml"
                "awsJson1_0" -> "application/x-amz-json-1.0"
                "awsJson1_1" -> "application/x-amz-json-1.1"
                else -> "application/json"
            }
        val eventMediaType =
            if (xml) {
                "application/xml"
            } else if (cbor) {
                "application/cbor"
            } else {
                "application/json"
            }
        val notePayload =
            when {
                cbor -> """b"\xbf\x64text\x62ok\xff".as_slice()"""
                xml -> """b"<Note><text>ok</text></Note>".as_slice()"""
                else -> """br##"{"text":"ok"}"##.as_slice()"""
            }
        val errorPayload =
            when {
                cbor -> """b"\xbf\x67message\x66broken\xff".as_slice()"""
                xml -> """b"<ErrorResponse><Error><message>broken</message></Error></ErrorResponse>".as_slice()"""
                else -> """br##"{"message":"broken"}"##.as_slice()"""
            }
        // Output error envelopes and discriminators were observed on generated serde servers.
        val outputErrorPayload =
            when (protocol) {
                "rpcv2Cbor" -> """b"\xbf\x66__type\x78\x20test.streamfailure##StreamFailure\x67message\x66broken\xff".as_slice()"""
                "restXml" -> """b"<Error><message>broken</message></Error>".as_slice()"""
                "awsJson1_0" -> """br##"{"message":"broken","__type":"test.streamfailure##StreamFailure"}"##.as_slice()"""
                "awsJson1_1" -> """br##"{"message":"broken","__type":"StreamFailure"}"##.as_slice()"""
                else -> """br##"{"message":"broken"}"##.as_slice()"""
            }
        val afterErrorPayload =
            when {
                cbor -> """b"\xbf\x64text\x6bafter-error\xff".as_slice()"""
                xml -> """b"<Note><text>after-error</text></Note>".as_slice()"""
                else -> """br##"{"text":"after-error"}"##.as_slice()"""
            }
        val expectedResponseBody =
            when {
                cbor ->
                    """
                    match expected_outcome {
                        "eof" => b"\xbf\x67summary\x6cevents=1;eof\xff".to_vec(),
                        "response-error" => b"\xbf\x67summary\x77events=1;response-error\xff".to_vec(),
                        "dispatch-failure" => b"\xbf\x67summary\x78\x19events=1;dispatch-failure\xff".to_vec(),
                        "service-error" => b"\xbf\x67summary\x76events=1;service-error\xff".to_vec(),
                        _ => unreachable!(),
                    }
                    """.trimIndent()
                xml -> """format!("<ReadOutput><summary>events=1;{expected_outcome}</summary></ReadOutput>").into_bytes()"""
                else -> """format!("{{\"summary\":\"events=1;{expected_outcome}\"}}").into_bytes()"""
            }
        val model =
            """
            ${'$'}version: "2"
            namespace test.streamfailure
            use $namespace#$protocol
            @$protocol service Frames { version: "1", operations: [Read, Watch] }
            @http(method: "POST", uri: "/read")
            operation Read {
                input := { @httpPayload events: Events }
                output := { summary: String }
            }
            @http(method: "POST", uri: "/watch")
            operation Watch { input := {}, output := { @httpPayload events: Events } }
            @streaming union Events { note: Note, failure: StreamFailure }
            structure Note { text: String }
            @error("server") structure StreamFailure { message: String }
            """.asSmithyModel()
        for (schemaSerde in listOf(false, true)) {
            val snapshot = run.resolve("$schemaSerde.txt")
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ServerAdditionalSettings.builder().withHttp1x().schemaSerde(schemaSerde).toObjectNode(),
                    cargoCommand = "cargo test --quiet --lib",
                ),
                testCoverage = HttpTestType.Default,
            ) { context, crate ->
                check(context.usesSchemaHttpSerde == schemaSerde)

                fun returned(
                    name: String,
                    output: String,
                ): String =
                    if (context.model.expectShape(ShapeId.from("test.streamfailure#$name"), OperationShape::class.java).errors.isEmpty()) {
                        output
                    } else {
                        "#{Ok}::<_, crate::error::${name}Error>($output)"
                    }
                val readOutput =
                    returned("Read", "crate::output::ReadOutput::builder().summary(#{Some}(summary)).build()")
                val watchOutput = returned("Watch", "crate::output::WatchOutput { events }")
                crate.testModule {
                    tokioTest("stream_failures_after_a_valid_event") {
                        rustTemplate(
                            """
                            let app = crate::Frames::builder(crate::FramesConfig::builder().build())
                                .read(|mut input: crate::input::ReadInput| async move {
                                    let mut count = 0;
                                    let outcome = loop {
                                        match input.events.recv().await {
                                            #{Ok}(#{Some}(crate::model::Events::Note(note))) => {
                                                assert_eq!(note.text.as_deref(), #{Some}("ok"));
                                                count += 1;
                                            },
                                            #{Ok}(#{None}) => break "eof",
                                            #{Err}(#{SdkError}::ResponseError(_)) => break "response-error",
                                            #{Err}(#{SdkError}::DispatchFailure(_)) => break "dispatch-failure",
                                            #{Err}(#{SdkError}::ServiceError(error)) => {
                                                match error.err() {
                                                    crate::error::EventsError::StreamFailure(error) =>
                                                        assert_eq!(error.message.as_deref(), #{Some}("broken")),
                                                }
                                                break "service-error";
                                            },
                                            #{Err}(other) => panic!("unexpected receiver error: {other:?}"),
                                        }
                                    };
                                    let summary = format!("events={count};{outcome}");
                                    $readOutput
                                })
                                .watch(|_: crate::input::WatchInput| async {
                                    let events: #{Sender}<crate::model::Events, crate::error::EventsError> =
                                        #{Futures}::stream::iter([
                                            #{Ok}(crate::model::Events::Note(crate::model::Note { text: #{Some}("ok".into()) })),
                                            #{Err}(crate::error::EventsError::StreamFailure(crate::error::StreamFailure {
                                                message: #{Some}("broken".into()),
                                            })),
                                            // Observe whether the sender emits events after a modeled error.
                                            #{Ok}(crate::model::Events::Note(crate::model::Note { text: #{Some}("after-error".into()) })),
                                        ]).into();
                                    $watchOutput
                                }).build().unwrap();
                            let encode = |message_type: &str, type_name: &str, payload: &[u8]| {
                                let message = #{Types}::event_stream::Message::new_from_parts(vec![
                                    #{Types}::event_stream::Header::new(":message-type", #{Types}::event_stream::HeaderValue::String(message_type.to_owned().into())),
                                    #{Types}::event_stream::Header::new(if message_type == "exception" { ":exception-type" } else { ":event-type" },
                                        #{Types}::event_stream::HeaderValue::String(type_name.to_owned().into())),
                                    #{Types}::event_stream::Header::new(":content-type", #{Types}::event_stream::HeaderValue::String("$eventMediaType".into())),
                                ], #{Bytes}::copy_from_slice(payload));
                                let mut bytes = #{Vec}::new();
                                #{EventStream}::frame::write_message_to(&message, &mut bytes).unwrap();
                                bytes
                            };
                            let good = encode("event", "note", $notePayload);
                            let mut prefix = #{Vec}::new();
                            ${if (rpc) {
                                """prefix.extend(encode("event", "initial-request", ${if (cbor) {
                                    """b"\xbf\xff".as_slice()"""
                                } else {
                                    """b"{}".as_slice()"""
                                }}));"""
                            } else {
                                ""
                            }}
                            prefix.extend_from_slice(&good);
                            let mut bad_prelude = good.clone(); bad_prelude[8] ^= 1;
                            let mut bad_message = good.clone(); let last = bad_message.len() - 1; bad_message[last] ^= 1;
                            let mut cases = vec![
                                ("eof", #{Vec}::new(), false, "eof"),
                                ("prelude-crc", bad_prelude, false, "response-error"),
                                ("message-crc", bad_message, false, "response-error"),
                                ("truncated-prelude", good[..8].to_vec(), false, "response-error"),
                                ("truncated-message", good[..good.len() - 1].to_vec(), false, "response-error"),
                                ("invalid-payload", encode("event", "note", b"invalid"), false, "response-error"),
                                ("unknown-exception", encode("exception", "StreamFailure", $errorPayload), false, "response-error"),
                                ("modeled-error", encode("exception", "failure", $errorPayload), false, "service-error"),
                                ("transport-failure", #{Vec}::new(), true, "dispatch-failure"),
                            ];
                            let missing_type = #{Types}::event_stream::Message::new_from_parts(vec![
                                #{Types}::event_stream::Header::new(":event-type", #{Types}::event_stream::HeaderValue::String("note".into())),
                            ], $notePayload);
                            let mut missing = #{Vec}::new();
                            #{EventStream}::frame::write_message_to(&missing_type, &mut missing).unwrap();
                            cases.push(("missing-message-type", missing, false, "response-error"));
                            let wrong_type = #{Types}::event_stream::Message::new_from_parts(vec![
                                #{Types}::event_stream::Header::new(":message-type", #{Types}::event_stream::HeaderValue::String("event".into())),
                                #{Types}::event_stream::Header::new(":event-type", #{Types}::event_stream::HeaderValue::Int32(7)),
                            ], $notePayload);
                            let mut wrong = #{Vec}::new();
                            #{EventStream}::frame::write_message_to(&wrong_type, &mut wrong).unwrap();
                            cases.push(("non-string-event-type", wrong, false, "response-error"));
                            let mut records = #{String}::new();
                            for (case, suffix, fail_transport, expected_outcome) in cases {
                                let mut payload = prefix.clone(); payload.extend(suffix);
                                let mut chunks = vec![#{Ok}(#{HttpBody}::Frame::data(#{Bytes}::from(payload)))];
                                if fail_transport { chunks.push(#{Err}(std::io::Error::other("stream transport failed"))); }
                                let request = #{Http}::Request::post(${if (cbor) {
                                "\"/service/Frames/operation/Read\""
                            } else if (rpc) {
                                "\"/\""
                            } else {
                                "\"/read\""
                            }})
                                    .header("content-type", "application/vnd.amazon.eventstream")
                                    ${if (cbor) {
                                """.header("smithy-protocol", "rpc-v2-cbor")"""
                            } else if (rpc) {
                                """.header("x-amz-target", "Frames.Read")"""
                            } else {
                                ""
                            }}
                                    .body(#{Server}::body::boxed_sync(#{BodyUtil}::StreamBody::new(#{Futures}::stream::iter(chunks)))).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(app.clone(), request).await.unwrap();
                                let status = response.status().as_u16();
                                let mut headers: #{Vec}<_> = response.headers().iter()
                                    .map(|(name, value)| (name.to_string(), value.as_bytes().to_vec())).collect();
                                headers.sort();
                                #{Collect}
                                if ${!schemaSerde} {
                                    // Literal wire expectations pinned after replaying the generated serde receiver.
                                    let expected_body = $expectedResponseBody;
                                    assert_eq!(status, 200, "{case}");
                                    assert_eq!(body.as_ref(), expected_body, "{case}");
                                    let mut expected_headers = vec![
                                        ("content-length".to_owned(), expected_body.len().to_string().into_bytes()),
                                        ("content-type".to_owned(), b"$mediaType".to_vec()),
                                    ];
                                    ${if (cbor) {
                                """expected_headers.push(("smithy-protocol".to_owned(), b"rpc-v2-cbor".to_vec()));"""
                            } else {
                                ""
                            }}
                                    expected_headers.sort();
                                    assert_eq!(headers, expected_headers, "{case}");
                                }
                                records.push_str(&format!("{case}: {status} headers={headers:?} body={:?}\n", body.as_ref()));
                            }
                            // A response-side modeled error is sent as an exception frame after one event.
                            let payload = ${if (cbor) {
                                """b"\xbf\xff".as_slice()"""
                            } else if (xml) {
                                """b"".as_slice()"""
                            } else {
                                """b"{}".as_slice()"""
                            }};
                            let request = #{Http}::Request::post(${if (cbor) {
                                "\"/service/Frames/operation/Watch\""
                            } else if (rpc) {
                                "\"/\""
                            } else {
                                "\"/watch\""
                            }})
                                .header("content-type", "$mediaType")
                                ${if (cbor) {
                                """.header("smithy-protocol", "rpc-v2-cbor")"""
                            } else if (rpc) {
                                """.header("x-amz-target", "Frames.Watch")"""
                            } else {
                                ""
                            }}
                                .body(#{Server}::body::to_boxed_sync(payload)).unwrap();
                            let response = #{Tower}::ServiceExt::oneshot(app, request).await.unwrap();
                            let status = response.status().as_u16();
                            let mut headers: #{Vec}<_> = response.headers().iter()
                                .map(|(name, value)| (name.to_string(), value.as_bytes().to_vec())).collect();
                            headers.sort();
                            #{Collect}
                            let mut remaining = body.as_ref();
                            let mut frame_types = #{Vec}::new();
                            let mut frames = #{Vec}::new();
                            while !remaining.is_empty() {
                                let frame = #{EventStream}::frame::read_message_from(&mut remaining).unwrap();
                                let header = |name| frame.headers().iter().find(|h| h.name().as_str() == name)
                                    .map(|h| h.value().as_string().unwrap().as_str().to_owned());
                                frame_types.push((header(":message-type"), header(":event-type"), header(":exception-type")));
                                frames.push(frame);
                            }
                            if ${!schemaSerde} {
                                assert_eq!(status, 200);
                                let mut expected_headers = vec![
                                    ("content-type".to_owned(), b"${if (protocol.startsWith("awsJson")) {
                                mediaType
                            } else {
                                "application/vnd.amazon.eventstream"
                            }}".to_vec()),
                                ];
                                ${if (cbor) {
                                """expected_headers.push(("smithy-protocol".to_owned(), b"rpc-v2-cbor".to_vec()));"""
                            } else {
                                ""
                            }}
                                expected_headers.sort();
                                assert_eq!(headers, expected_headers);
                                // Generated serde sends the next event after the exception; no initial frame by default.
                                let expected_frames = [
                                    ("event", ":event-type", "note", $notePayload),
                                    ("exception", ":exception-type", "failure", $outputErrorPayload),
                                    ("event", ":event-type", "note", $afterErrorPayload),
                                ];
                                assert_eq!(frames.len(), expected_frames.len());
                                for (frame, (message_type, type_header, type_name, payload)) in frames.iter().zip(expected_frames) {
                                    assert_eq!(frame.payload().as_ref(), payload);
                                    let mut headers: #{Vec}<_> = frame.headers().iter().map(|header|
                                        (header.name().as_str(), header.value().as_string().unwrap().as_str())).collect();
                                    headers.sort();
                                    let mut expected = vec![
                                        (":message-type", message_type), (type_header, type_name), (":content-type", "$eventMediaType"),
                                    ];
                                    expected.sort();
                                    assert_eq!(headers, expected);
                                }
                            }
                            records.push_str(&format!("output: {status} headers={headers:?} frames={frame_types:?} body={:?}\n", body.as_ref()));
                            std::fs::write(${snapshot.toString().dq()}, records).unwrap();
                            """,
                            *RuntimeType.preludeScope,
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                            "HttpBody" to CargoDependency.HttpBody1x.toType(),
                            "Futures" to CargoDependency.FuturesUtil.toType(),
                            "Bytes" to RuntimeType.Bytes,
                            "Types" to RuntimeType.smithyTypes(context.runtimeConfig),
                            "EventStream" to RuntimeType.smithyEventStream(context.runtimeConfig),
                            "SdkError" to RuntimeType.smithyRuntimeApiClient(context.runtimeConfig).resolve("client::result::SdkError"),
                            "Sender" to RuntimeType.smithyHttp(context.runtimeConfig).resolve("event_stream::EventStreamSender"),
                            "Collect" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "body", "response"),
                        )
                    }
                }
            }
        }
        assertEquals(Files.readString(run.resolve("false.txt")), Files.readString(run.resolve("true.txt")), run.toString())
    }
}
