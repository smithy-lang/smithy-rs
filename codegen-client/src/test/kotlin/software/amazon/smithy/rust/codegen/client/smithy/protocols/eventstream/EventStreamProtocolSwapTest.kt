/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols.eventstream

import org.junit.jupiter.api.Assumptions.assumeTrue
import org.junit.jupiter.api.Test
import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.protocol.traits.Rpcv2CborTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customizations.SchemaSerdeAllowlist
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.dq
import kotlin.io.path.readText

/**
 * Event-stream protocol swapping: both halves of an event-stream request must follow the protocol
 * selected at runtime, not the one the client was generated for.
 *
 * [ProtocolSwapMatrixTest][software.amazon.smithy.rust.codegen.client.smithy.protocols.ProtocolSwapMatrixTest]
 * covers non-event-stream requests. Event streams were deliberately excluded from it, and that
 * exclusion left two values unpinned, both of which this test pins:
 *
 * 1. **The per-event `:content-type`.** Event payload *bytes* have always been encoded by the
 *    runtime protocol's codec (`payload_codec()`), but the label announcing them was a codegen
 *    literal keyed on the generated protocol. After a swap the frame contradicted itself — a JSON
 *    payload announced as `application/cbor` — so a peer honouring the header decodes with the
 *    wrong codec. That is a hard decode failure rather than a cosmetic mismatch, which makes it
 *    the most severe item in this class.
 *
 *    Note the existing `EventStreamMarshallTestCases` assertions already pin this value for the
 *    *unswapped* case, and pin it through a real `SharedClientProtocol`. What they cannot catch is
 *    a swap, because there the generated protocol and the selected protocol agree by construction.
 *    This test supplies only the missing dimension.
 *
 * 2. **The request-level framing of an event-stream operation** — `smithy-protocol` and the
 *    event-stream `accept`. `accept` is the interesting one because it is genuinely *shared*
 *    ownership: the eventstream marker is a model fact (this operation's output is an event
 *    stream, which `serialize_request` cannot see because it receives only the input schema) while
 *    the media type after it is a protocol fact. So codegen must contribute the marker and the
 *    runtime must contribute the media type, and only the composition is correct.
 *
 * The operation used for (2) is **output-only** on purpose. It needs no `EventStreamSender` to
 * drive, and it exercises the branch that keys on `isOutputEventStream` rather than on the input
 * having a streaming member — which is the pairing that a previous refactor got wrong, sending a
 * plain `application/cbor` for output-only streams.
 */
class EventStreamProtocolSwapTest {
    /**
     * @param name used for the generated test function name.
     * @param construct the Rust expression selecting the protocol.
     * @param eventContentType the media type the protocol must label a structured event payload
     *   with. `null` for protocols that declare none, which are excluded from the marshaller test.
     * @param accept expected `accept` on an output-event-stream request; `null` means the header
     *   must be absent entirely.
     * @param smithyProtocol expected `smithy-protocol`; `null` means absent.
     */
    private data class Target(
        val name: String,
        val construct: String,
        val eventContentType: String?,
        val accept: String?,
        val smithyProtocol: String? = null,
    )

    private val targets =
        listOf(
            Target(
                name = "rpcv2cbor",
                construct = "#{RpcV2CborProtocol}::new()",
                eventContentType = "application/cbor",
                // The composition under test: codegen's marker + the runtime protocol's media type.
                accept = "application/vnd.amazon.eventstream, application/cbor",
                smithyProtocol = "rpc-v2-cbor",
            ),
            Target(
                name = "awsjson10",
                construct = "#{AwsJsonRpcProtocol}::aws_json_1_0()",
                eventContentType = "application/json",
                // awsJson sets no `accept`, so the upgrade has nothing to prepend to and must not
                // invent one. Asserting absence here is what catches a stale cbor `accept`.
                accept = null,
            ),
            Target(
                name = "restjson1",
                construct = "#{AwsRestJsonProtocol}::new()",
                eventContentType = "application/json",
                accept = null,
            ),
            Target(
                name = "restxml",
                construct = "#{AwsRestXmlProtocol}::new()",
                eventContentType = "application/xml",
                accept = null,
            ),
            // Declares no event-stream media type and sets no `accept`; included in the request
            // half only, as a control that a swap to a protocol without event-stream support still
            // strips the generated protocol's framing.
            Target(
                name = "awsquery",
                construct = "#{AwsQueryProtocol}::new()",
                eventContentType = null,
                accept = null,
            ),
        )

    private val allFramingHeaders = listOf("smithy-protocol", "accept")

    private fun protocolScope(runtimeConfig: RuntimeConfig) =
        arrayOf(
            "RpcV2CborProtocol" to RuntimeType.smithyCbor(runtimeConfig).resolve("protocol::RpcV2CborProtocol"),
            "AwsJsonRpcProtocol" to
                RuntimeType.smithyJson(runtimeConfig).resolve("protocol::aws_json_rpc::AwsJsonRpcProtocol"),
            "AwsRestJsonProtocol" to
                RuntimeType.smithyJson(runtimeConfig).resolve("protocol::aws_rest_json_1::AwsRestJsonProtocol"),
            "AwsRestXmlProtocol" to
                RuntimeType.smithyXml(runtimeConfig).resolve("protocol::aws_rest_xml::AwsRestXmlProtocol"),
            "AwsQueryProtocol" to RuntimeType.smithyQuery(runtimeConfig).resolve("protocol::AwsQueryProtocol"),
            "SharedClientProtocol" to
                RuntimeType.smithySchema(runtimeConfig).resolve("protocol::SharedClientProtocol"),
            "MarshallMessage" to RuntimeType.smithyEventStream(runtimeConfig).resolve("frame::MarshallMessage"),
            "UnmarshallMessage" to
                RuntimeType.smithyEventStream(runtimeConfig).resolve("frame::UnmarshallMessage"),
            "UnmarshalledMessage" to
                RuntimeType.smithyEventStream(runtimeConfig).resolve("frame::UnmarshalledMessage"),
            "Message" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::Message"),
            "Header" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::Header"),
            "HeaderValue" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::HeaderValue"),
            "ProvideErrorMetadata" to
                RuntimeType.smithyTypes(runtimeConfig).resolve("error::metadata::ProvideErrorMetadata"),
        )

    /**
     * Two operations, because the two halves need different shapes: a marshaller is only generated
     * for an *input* event stream, while the `accept` upgrade keys on an *output* one. Separate
     * unions avoid the synthetic per-direction unions the event-stream normalizer would otherwise
     * introduce for a single union used in both positions.
     */
    private fun model(protocolAnnotation: String) =
        """
        namespace test

        @$protocolAnnotation
        @xmlNamespace(uri: "http://example.com/eventswap/")
        service EventSwapService {
            version: "2024-01-01",
            operations: [ReceiveStats, SendStats]
        }

        @http(method: "POST", uri: "/receive")
        operation ReceiveStats {
            input := { name: String }
            output := {
                @httpPayload
                events: OutEvents
            }
        }

        @http(method: "POST", uri: "/send")
        operation SendStats {
            input := {
                @httpPayload
                events: InEvents
            }
            output := { value: String }
        }

        @streaming
        union OutEvents { stats: StatsEvent }

        @streaming
        union InEvents { stats: StatsEvent }

        structure StatsEvent { value: String }
        """.asSmithyModel(smithyVersion = "2.0")

    private fun swapTestsFor(
        protocolAnnotation: String,
        protocolId: ShapeId,
    ) {
        assumeTrue(
            SchemaSerdeAllowlist.isProtocolEnabled(protocolId),
            "$protocolId is not on SchemaSerdeAllowlist, so the schema-serde event-stream path is not generated",
        )
        val testDir =
            clientIntegrationTest(model(protocolAnnotation)) { context: ClientCodegenContext, rustCrate ->
                rustCrate.testModule {
                    val scope = protocolScope(context.runtimeConfig)

                    // (1) The per-event `:content-type` must come from the selected protocol.
                    targets.filter { it.eventContentType != null }.forEach { target ->
                        unitTest("event_content_type_follows_${target.name}") {
                            rustTemplate(
                                """
                                let marshaller = crate::event_stream_serde::InEventsMarshaller::new(
                                    #{SharedClientProtocol}::new(${target.construct}),
                                );
                                let event = crate::types::InEvents::Stats(
                                    crate::types::StatsEvent::builder().value("v").build(),
                                );
                                let message = #{MarshallMessage}::marshall(&marshaller, event)
                                    .expect("the event must marshall");
                                let content_type = message
                                    .headers()
                                    .iter()
                                    .find(|header| header.name().as_str() == ":content-type")
                                    .expect("every structured event frame carries a :content-type")
                                    .value()
                                    .as_string()
                                    .expect(":content-type must be a string header")
                                    .as_str()
                                    .to_string();
                                assert_eq!(
                                    ${target.eventContentType!!.dq()},
                                    content_type,
                                    "the per-event :content-type must follow the protocol selected at \
                                     runtime, not the one this client was generated for — otherwise the \
                                     frame announces a codec that did not encode its payload",
                                );
                                """,
                                *RuntimeType.preludeScope,
                                *scope,
                            )
                        }
                    }

                    // (3) The event-stream *error* envelope must be parsed by the selected protocol
                    // too. Its payload already decoded through the runtime codec, so a codegen-selected
                    // envelope parser meant the discriminator was read with the wrong format after a
                    // swap — the parse fails, the frame degrades to an unhandled error, and the error
                    // code is lost along with the modeled variant and any retry classification keyed
                    // on it.
                    //
                    // restJson1 is the selected protocol here because the envelope has to be
                    // hand-encoded and JSON is the format that can be written literally. For every
                    // generated client except the restJson1 one this is a genuine swap.
                    unitTest("event_error_envelope_parsed_by_selected_protocol") {
                        rustTemplate(
                            """
                            let unmarshaller = crate::event_stream_serde::OutEventsUnmarshaller::new(
                                #{SharedClientProtocol}::new(#{AwsRestJsonProtocol}::new()),
                            );
                            let message = #{Message}::new(
                                &b"{\"__type\":\"BadEvent\",\"message\":\"went wrong\"}"[..],
                            )
                            .add_header(#{Header}::new(
                                ":message-type",
                                #{HeaderValue}::String("exception".into()),
                            ))
                            .add_header(#{Header}::new(
                                ":exception-type",
                                #{HeaderValue}::String("BadEvent".into()),
                            ))
                            .add_header(#{Header}::new(
                                ":content-type",
                                #{HeaderValue}::String("application/json".into()),
                            ));

                            let result = #{UnmarshallMessage}::unmarshall(&unmarshaller, &message)
                                .expect("an exception frame must unmarshall");
                            match result {
                                #{UnmarshalledMessage}::Error(err) => assert_eq!(
                                    #{Some}("BadEvent"),
                                    #{ProvideErrorMetadata}::code(&err),
                                    "the error envelope must be parsed by the protocol selected at \
                                     runtime; losing the code degrades a modeled error to unhandled",
                                ),
                                other => panic!("expected an error frame, got {other:?}"),
                            }
                            """,
                            *RuntimeType.preludeScope,
                            *scope,
                        )
                    }

                    // (2) Request-level event-stream framing must come from the selected protocol.
                    targets.forEach { target ->
                        val expectations =
                            allFramingHeaders.joinToString("\n") { header ->
                                val expected = if (header == "accept") target.accept else target.smithyProtocol
                                val expectedExpr = if (expected == null) "#{None}" else "#{Some}(${expected.dq()})"
                                val why =
                                    if (expected == null) {
                                        "$header belongs to another protocol and must not survive the swap"
                                    } else {
                                        "the selected protocol must own $header"
                                    }
                                """
                                assert_eq!(
                                    $expectedExpr,
                                    request.headers().get(${header.dq()}),
                                    ${why.dq()},
                                );
                                """.trimIndent()
                            }
                        tokioTest("output_event_stream_framing_swap_to_${target.name}") {
                            rustTemplate(
                                """
                                let (http_client, rx) = #{capture_request}(#{None});
                                let config = crate::Config::builder()
                                    .http_client(http_client)
                                    .endpoint_url("http://localhost:1234")
                                    .behavior_version_latest()
                                    .protocol(${target.construct})
                                    .build();
                                let client = crate::Client::from_conf(config);

                                let _ = client.receive_stats().name("test").send().await;
                                let request = rx.expect_request();

                                $expectations
                                """,
                                *RuntimeType.preludeScope,
                                *scope,
                                "capture_request" to RuntimeType.captureRequest(context.runtimeConfig),
                            )
                        }
                    }
                }
            }

        // The wire-level tests above drive `ReceiveStats`, whose stream is on the *output*, so it
        // takes the standard request branch. `SendStats` has the stream on its *input* and takes
        // the event-stream branch, which is where framing literals were previously emitted — and
        // reaching its wire would require driving an `EventStreamSender`. Assert on the emitted
        // source instead, which pins that branch directly.
        val sendStats = testDir.resolve("src/operation/send_stats.rs").readText()
        listOf("smithy-protocol", "accept").forEach { header ->
            check(!sendStats.contains("""insert("$header", """")) {
                "the event-stream request branch must not emit a literal `$header`; the runtime " +
                    "protocol owns it, and a literal survives a swap. Found in send_stats.rs"
            }
        }
    }

    @Test
    fun `rpcv2Cbor event streams honor every runtime-selected protocol`() =
        swapTestsFor("smithy.protocols#rpcv2Cbor", Rpcv2CborTrait.ID)

    @Test
    fun `restJson1 event streams honor every runtime-selected protocol`() =
        swapTestsFor("aws.protocols#restJson1", RestJson1Trait.ID)

    @Test
    fun `awsJson1_0 event streams honor every runtime-selected protocol`() =
        swapTestsFor("aws.protocols#awsJson1_0", AwsJson1_0Trait.ID)

    @Test
    fun `restXml event streams honor every runtime-selected protocol`() =
        swapTestsFor("aws.protocols#restXml", RestXmlTrait.ID)
}
