/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import io.kotest.matchers.string.shouldContain
import io.kotest.matchers.string.shouldNotContain
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import java.nio.file.Files
import kotlin.io.path.extension
import kotlin.io.path.readText

/**
 * The operation-scoped `DocumentUnionDecodeMode`: a response reads the original union variant by default, and the
 * `DocumentV2` variant for every affected member, at any depth, when the caller selects `V2`.
 */
class DocumentUnionDecodeModeTest {
    private val model =
        """
        ${'$'}version: "2"
        namespace $SCHEMA_SERDE_NAMESPACE

        use aws.protocols#restJson1

        @restJson1
        service ChoiceService {
            version: "1"
            operations: [GetChoice, ListChoices, StreamChoices, PutText, SendChoices]
        }

        @http(uri: "/choice", method: "POST")
        operation GetChoice {
            input := {
                choice: Choice
            }
            output := {
                choice: Choice
                choices: ChoiceList
                holder: Holder
                outer: Outer
            }
            errors: [ChoiceError]
        }

        @readonly
        @paginated(inputToken: "nextToken", outputToken: "nextToken", items: "choices")
        @http(uri: "/choices", method: "GET")
        operation ListChoices {
            input := {
                @httpQuery("nextToken")
                nextToken: String
            }
            output := {
                nextToken: String
                choices: ChoiceList
            }
        }

        @http(uri: "/stream", method: "POST")
        operation StreamChoices {
            output := {
                @httpPayload
                events: Events
            }
        }

        @http(uri: "/text", method: "POST")
        operation PutText {
            input := {
                text: TextOnly
            }
            output := {
                text: TextOnly
            }
        }

        // A request-only event stream, so its operation gets no mode.
        @http(uri: "/send", method: "POST")
        operation SendChoices {
            input := {
                @httpPayload
                events: InputEvents
            }
        }

        @streaming
        union Events {
            tick: Tick
            wrapped: Wrapped
            direct: Direct
            failure: StreamFailure
        }

        @streaming
        union InputEvents {
            tick: Tick
            direct: Direct
        }

        structure Tick {
            choice: Choice
        }

        // An event whose payload is a structure that holds an affected union.
        structure Wrapped {
            @eventPayload
            holder: Holder
        }

        // An event whose payload is an affected union.
        structure Direct {
            @eventPayload
            choice: Choice
        }

        @error("server")
        structure StreamFailure {
            message: String
            choice: Choice
        }

        union Choice {
            doc: Document
            docs: DocumentList
            text: String
        }

        union TextOnly {
            text: String
        }

        union Outer {
            holder: Holder
            text: String
        }

        structure Holder {
            choice: Choice
        }

        list ChoiceList {
            member: Choice
        }

        list DocumentList {
            member: Document
        }

        @error("client")
        @httpError(400)
        structure ChoiceError {
            message: String
            choice: Choice
        }
        """.asSmithyModel()

    @Test
    fun `responses read DocumentV2 union variants only when the operation selects V2`() {
        val crateDir =
            clientIntegrationTest(model) { codegenContext, rustCrate ->
                val runtimeConfig = codegenContext.runtimeConfig
                rustCrate.testModule {
                    rustTemplate(
                        """
                        use crate::config::DocumentUnionDecodeMode;
                        use crate::types::{Choice, Outer};
                        use #{BigInteger};
                        use #{Document};
                        use #{DocumentV2};
                        use #{Number};
                        use std::str::FromStr;

                        const BIG: &str = "123456789012345678901234567890";

                        fn response(status: u16, headers: &[(&str, &str)], body: impl Into<#{SdkBody}>) -> #{http_1x}::Response<#{SdkBody}> {
                            let mut builder = #{http_1x}::Response::builder().status(status);
                            for (name, value) in headers {
                                builder = builder.header(*name, *value);
                            }
                            builder.body(body.into()).unwrap()
                        }

                        fn client(responses: Vec<#{http_1x}::Response<#{SdkBody}>>) -> crate::Client {
                            let events = responses
                                .into_iter()
                                .map(|response| {
                                    #{ReplayEvent}::new(
                                        #{http_1x}::Request::builder()
                                            .uri("http://localhost:1234/")
                                            .body(#{SdkBody}::empty())
                                            .unwrap(),
                                        response,
                                    )
                                })
                                .collect();
                            let config = crate::Config::builder()
                                .endpoint_url("http://localhost:1234")
                                .http_client(#{StaticReplayClient}::new(events))
                                .behavior_version_latest()
                                .build();
                            crate::Client::from_conf(config)
                        }

                        // Every affected member, directly, in a list, in a structure, and in another union.
                        fn get_choice_body() -> String {
                            format!(
                                r##"{{"choice":{{"doc":{BIG}}},"choices":[{{"doc":2}},{{"text":"t"}}],"holder":{{"choice":{{"docs":[3]}}}},"outer":{{"holder":{{"choice":{{"doc":4}}}}}}}}"##
                            )
                        }

                        fn v1(value: u64) -> Document {
                            Document::Number(Number::PosInt(value))
                        }

                        fn v2(value: u64) -> DocumentV2 {
                            DocumentV2::from(value)
                        }

                        fn big() -> DocumentV2 {
                            DocumentV2::BigInteger(BigInteger::from_str(BIG).unwrap())
                        }

                        fn outer_choice(outer: &Option<Outer>) -> &Choice {
                            outer.as_ref().unwrap().as_holder().unwrap().choice.as_ref().unwrap()
                        }

                        #{TokioTest}
                        async fn responses_read_the_original_variant_by_default() {
                            let client = client(vec![response(200, &[], get_choice_body())]);
                            let output = client.get_choice().send().await.unwrap();
                            assert!(output.choice.as_ref().unwrap().is_doc());
                            assert_eq!(output.choices.as_ref().unwrap()[0], Choice::Doc(v1(2)));
                            assert_eq!(output.choices.as_ref().unwrap()[1], Choice::Text("t".into()));
                            assert_eq!(output.holder.unwrap().choice, Some(Choice::Docs(vec![v1(3)])));
                            assert_eq!(outer_choice(&output.outer), &Choice::Doc(v1(4)));
                        }

                        #{TokioTest}
                        async fn v2_reads_the_document_v2_variant_for_every_affected_member() {
                            let client = client(vec![response(200, &[], get_choice_body())]);
                            let output = client
                                .get_choice()
                                .document_union_decode_mode(DocumentUnionDecodeMode::V2)
                                .send()
                                .await
                                .unwrap();
                            // The exact number survives, and values a `Document` could hold still use the
                            // `DocumentV2` variant.
                            assert_eq!(output.choice, Some(Choice::DocV2(big())));
                            assert_eq!(output.choices.as_ref().unwrap()[0], Choice::DocV2(v2(2)));
                            // A member without documents is unchanged.
                            assert_eq!(output.choices.as_ref().unwrap()[1], Choice::Text("t".into()));
                            assert_eq!(output.holder.unwrap().choice, Some(Choice::DocsV2(vec![v2(3)])));
                            assert_eq!(outer_choice(&output.outer), &Choice::DocV2(v2(4)));
                        }

                        #{TokioTest}
                        async fn an_explicit_v1_reads_the_original_variant() {
                            let client = client(vec![response(200, &[], get_choice_body())]);
                            let output = client
                                .get_choice()
                                .document_union_decode_mode(DocumentUnionDecodeMode::V1)
                                .send()
                                .await
                                .unwrap();
                            assert_eq!(output.choices.as_ref().unwrap()[0], Choice::Doc(v1(2)));
                        }

                        #{TokioTest}
                        async fn the_mode_applies_to_one_invocation() {
                            let client = client(vec![
                                response(200, &[], get_choice_body()),
                                response(200, &[], get_choice_body()),
                            ]);
                            let output = client
                                .get_choice()
                                .document_union_decode_mode(DocumentUnionDecodeMode::V2)
                                .send()
                                .await
                                .unwrap();
                            assert_eq!(output.choices.as_ref().unwrap()[0], Choice::DocV2(v2(2)));
                            let output = client.get_choice().send().await.unwrap();
                            assert_eq!(output.choices.as_ref().unwrap()[0], Choice::Doc(v1(2)));
                        }

                        #{TokioTest}
                        async fn customize_keeps_the_mode() {
                            let client = client(vec![response(200, &[], get_choice_body())]);
                            let output = client
                                .get_choice()
                                .document_union_decode_mode(DocumentUnionDecodeMode::V2)
                                .customize()
                                .config_override(crate::Config::builder())
                                .send()
                                .await
                                .unwrap();
                            assert_eq!(output.choices.as_ref().unwrap()[0], Choice::DocV2(v2(2)));
                        }

                        ##[test]
                        fn the_fluent_builder_stores_the_mode() {
                            let client = client(vec![]);
                            let builder = client.get_choice();
                            assert_eq!(builder.get_document_union_decode_mode(), &None);
                            let builder = builder.document_union_decode_mode(DocumentUnionDecodeMode::V2);
                            assert_eq!(builder.get_document_union_decode_mode(), &Some(DocumentUnionDecodeMode::V2));
                            let builder = builder.set_document_union_decode_mode(None);
                            assert_eq!(builder.get_document_union_decode_mode(), &None);
                        }

                        #{TokioTest}
                        async fn modeled_errors_read_through_the_mode() {
                            let error = || response(
                                400,
                                &[("x-amzn-errortype", "ChoiceError")],
                                r##"{"message":"oops","choice":{"doc":1}}"##,
                            );
                            let client = client(vec![error(), error()]);

                            let err = client.get_choice().send().await.unwrap_err().into_service_error();
                            let crate::operation::get_choice::GetChoiceError::ChoiceError(err) = err else {
                                panic!("expected ChoiceError, got {err:?}");
                            };
                            assert_eq!(err.choice, Some(Choice::Doc(v1(1))));

                            let err = client
                                .get_choice()
                                .document_union_decode_mode(DocumentUnionDecodeMode::V2)
                                .send()
                                .await
                                .unwrap_err()
                                .into_service_error();
                            let crate::operation::get_choice::GetChoiceError::ChoiceError(err) = err else {
                                panic!("expected ChoiceError, got {err:?}");
                            };
                            assert_eq!(err.choice, Some(Choice::DocV2(v2(1))));
                        }

                        #{TokioTest}
                        async fn paginators_read_every_page_through_the_mode() {
                            let client = client(vec![
                                response(200, &[], r##"{"nextToken":"next","choices":[{"doc":1}]}"##),
                                response(200, &[], r##"{"choices":[{"doc":2}]}"##),
                            ]);
                            let pages = client
                                .list_choices()
                                .document_union_decode_mode(DocumentUnionDecodeMode::V2)
                                .into_paginator()
                                .send()
                                .try_collect()
                                .await
                                .unwrap();
                            let choices: Vec<_> = pages.into_iter().flat_map(|page| page.choices.unwrap()).collect();
                            assert_eq!(choices, vec![Choice::DocV2(v2(1)), Choice::DocV2(v2(2))]);
                        }

                        fn frame(message_type: &str, type_header: &str, type_value: &str, body: &str) -> Vec<u8> {
                            let message = #{Message}::new_from_parts(
                                vec![
                                    #{Header}::new(":message-type", #{HeaderValue}::String(message_type.to_owned().into())),
                                    #{Header}::new(type_header.to_owned(), #{HeaderValue}::String(type_value.to_owned().into())),
                                    #{Header}::new(":content-type", #{HeaderValue}::String("application/json".into())),
                                ],
                                body.to_owned(),
                            );
                            let mut buffer = Vec::new();
                            #{write_message_to}(&message, &mut buffer).unwrap();
                            buffer
                        }

                        fn event_frame(event_type: &str, body: &str) -> Vec<u8> {
                            frame("event", ":event-type", event_type, body)
                        }

                        fn stream_response(frames: Vec<Vec<u8>>) -> #{http_1x}::Response<#{SdkBody}> {
                            response(200, &[("content-type", "application/vnd.amazon.eventstream")], frames.concat())
                        }

                        fn tick_stream(body: &str) -> Vec<u8> {
                            event_frame("tick", body)
                        }

                        #{TokioTest}
                        async fn event_stream_events_read_through_the_mode() {
                            let stream = || response(
                                200,
                                &[("content-type", "application/vnd.amazon.eventstream")],
                                tick_stream(r##"{"choice":{"doc":1}}"##),
                            );
                            let client = client(vec![stream(), stream()]);

                            let mut output = client.stream_choices().send().await.unwrap();
                            let event = output.events.recv().await.unwrap().unwrap();
                            assert_eq!(event.as_tick().unwrap().choice, Some(Choice::Doc(v1(1))));

                            let mut output = client
                                .stream_choices()
                                .document_union_decode_mode(DocumentUnionDecodeMode::V2)
                                .send()
                                .await
                                .unwrap();
                            let event = output.events.recv().await.unwrap().unwrap();
                            assert_eq!(event.as_tick().unwrap().choice, Some(Choice::DocV2(v2(1))));
                        }

                        #{TokioTest}
                        async fn event_payload_members_read_through_the_mode() {
                            // `wrapped`'s `@eventPayload` is a structure that holds an affected union, and
                            // `direct`'s is an affected union.
                            let stream = || stream_response(vec![
                                event_frame("wrapped", r##"{"choice":{"doc":1}}"##),
                                event_frame("direct", r##"{"docs":[2]}"##),
                            ]);
                            let client = client(vec![stream(), stream()]);

                            let mut output = client.stream_choices().send().await.unwrap();
                            let wrapped = output.events.recv().await.unwrap().unwrap();
                            let holder = wrapped.as_wrapped().unwrap().holder.as_ref().unwrap();
                            assert_eq!(holder.choice, Some(Choice::Doc(v1(1))));
                            let direct = output.events.recv().await.unwrap().unwrap();
                            assert_eq!(direct.as_direct().unwrap().choice, Some(Choice::Docs(vec![v1(2)])));

                            let mut output = client
                                .stream_choices()
                                .document_union_decode_mode(DocumentUnionDecodeMode::V2)
                                .send()
                                .await
                                .unwrap();
                            let wrapped = output.events.recv().await.unwrap().unwrap();
                            let holder = wrapped.as_wrapped().unwrap().holder.as_ref().unwrap();
                            assert_eq!(holder.choice, Some(Choice::DocV2(v2(1))));
                            let direct = output.events.recv().await.unwrap().unwrap();
                            assert_eq!(direct.as_direct().unwrap().choice, Some(Choice::DocsV2(vec![v2(2)])));
                        }

                        #{TokioTest}
                        async fn event_stream_errors_read_through_the_mode() {
                            let stream = || stream_response(vec![
                                event_frame("tick", r##"{"choice":{"doc":1}}"##),
                                frame("exception", ":exception-type", "failure", r##"{"message":"oops","choice":{"doc":2}}"##),
                            ]);
                            let client = client(vec![stream(), stream()]);

                            for (mode, tick, failure) in [
                                (None, Choice::Doc(v1(1)), Choice::Doc(v1(2))),
                                (Some(DocumentUnionDecodeMode::V2), Choice::DocV2(v2(1)), Choice::DocV2(v2(2))),
                            ] {
                                let mut output = client
                                    .stream_choices()
                                    .set_document_union_decode_mode(mode)
                                    .send()
                                    .await
                                    .unwrap();
                                // Events before the error use the same mode.
                                let event = output.events.recv().await.unwrap().unwrap();
                                assert_eq!(event.as_tick().unwrap().choice, Some(tick));
                                let err = output.events.recv().await.unwrap_err().into_service_error();
                                let crate::types::error::EventsError::StreamFailure(err) = err else {
                                    panic!("expected StreamFailure, got {err:?}");
                                };
                                assert_eq!(err.message.as_deref(), Some("oops"));
                                assert_eq!(err.choice, Some(failure));
                            }
                        }

                        ##[test]
                        fn input_events_write_document_v2_variants_under_the_modeled_member() {
                            use crate::types::{Direct, InputEvents, Tick};

                            let marshaller = crate::event_stream_serde::InputEventsMarshaller::new(
                                #{SharedClientProtocol}::new(#{AwsRestJsonProtocol}::new()),
                            );
                            let payload = |event: InputEvents| {
                                let message = #{MarshallMessage}::marshall(&marshaller, event).unwrap();
                                String::from_utf8(message.payload().to_vec()).unwrap()
                            };

                            // A structure event that holds an affected union, with each variant.
                            let tick = |choice| InputEvents::Tick(Tick::builder().choice(choice).build());
                            assert_eq!(payload(tick(Choice::DocV2(big()))), format!(r##"{{"choice":{{"doc":{BIG}}}}}"##));
                            assert_eq!(payload(tick(Choice::Doc(v1(1)))), r##"{"choice":{"doc":1}}"##);

                            // An affected union as the `@eventPayload`.
                            let direct = InputEvents::Direct(Direct::builder().choice(Choice::DocsV2(vec![big()])).build());
                            assert_eq!(payload(direct), format!(r##"{{"docs":[{BIG}]}}"##));
                        }
                        """,
                        "BigInteger" to RuntimeType.bigInteger(runtimeConfig),
                        "Document" to RuntimeType.document(runtimeConfig),
                        "DocumentV2" to RuntimeType.documentV2(runtimeConfig),
                        "Number" to RuntimeType.smithyTypes(runtimeConfig).resolve("Number"),
                        "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
                        "http_1x" to CargoDependency.Http1x.toType(),
                        "ReplayEvent" to
                            CargoDependency.smithyHttpClientTestUtil(runtimeConfig).toType()
                                .resolve("test_util::ReplayEvent"),
                        "StaticReplayClient" to
                            CargoDependency.smithyHttpClientTestUtil(runtimeConfig).toType()
                                .resolve("test_util::StaticReplayClient"),
                        "Message" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::Message"),
                        "Header" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::Header"),
                        "HeaderValue" to RuntimeType.smithyTypes(runtimeConfig).resolve("event_stream::HeaderValue"),
                        "write_message_to" to
                            RuntimeType.smithyEventStream(runtimeConfig).resolve("frame::write_message_to"),
                        "MarshallMessage" to RuntimeType.smithyEventStream(runtimeConfig).resolve("frame::MarshallMessage"),
                        "SharedClientProtocol" to
                            RuntimeType.smithySchema(runtimeConfig).resolve("protocol::SharedClientProtocol"),
                        "AwsRestJsonProtocol" to
                            RuntimeType.smithyJson(runtimeConfig).resolve("protocol::aws_rest_json_1::AwsRestJsonProtocol"),
                        // Renders `#[tokio::test]` and adds the `tokio` dev-dependency it needs.
                        "TokioTest" to writable { Attribute.TokioTest.render(this) },
                    )
                }
            }

        val operationSources = { operation: String ->
            Files.walk(crateDir.resolve("src/operation/$operation")).use { paths ->
                paths.filter { it.extension == "rs" }.map { it.readText() }.toList()
            }.joinToString("\n")
        }
        // An operation whose response can't reach an affected union gets no mode.
        operationSources("put_text") shouldNotContain "document_union_decode_mode"
        // Neither does one whose only affected union is in its request event stream.
        operationSources("send_choices") shouldNotContain "document_union_decode_mode"
        operationSources("get_choice") shouldContain "pub fn document_union_decode_mode("
        crateDir.resolve("src/config.rs").readText() shouldContain "pub use ::aws_smithy_schema::serde::DocumentUnionDecodeMode;"
    }

    @Test
    fun `a fluent builder method collision is an error`() {
        val model =
            """
            ${'$'}version: "2"
            namespace $SCHEMA_SERDE_NAMESPACE

            use aws.protocols#restJson1

            @restJson1
            service ChoiceService {
                version: "1"
                operations: [GetChoice]
            }

            @http(uri: "/choice", method: "POST")
            operation GetChoice {
                input := {
                    documentUnionDecodeMode: String
                }
                output := {
                    choice: Choice
                }
            }

            union Choice {
                doc: Document
            }
            """.asSmithyModel()

        val error = assertThrows<CodegenException> { clientIntegrationTest(model) }
        error.message!! shouldContain
            "needs a fluent builder method named `document_union_decode_mode` to select its " +
            "`DocumentUnionDecodeMode`, but input member `documentUnionDecodeMode` already has a method with that name"
    }

    @Test
    fun `an event body read by the protocol_serde parser can't reach an affected union`() {
        val model =
            """
            ${'$'}version: "2"
            namespace $SCHEMA_SERDE_NAMESPACE

            use aws.protocols#restJson1

            @restJson1
            service ChoiceService {
                version: "1"
                operations: [StreamChoices]
            }

            @http(uri: "/stream", method: "POST")
            operation StreamChoices {
                output := {
                    @httpPayload
                    events: Events
                }
            }

            @streaming
            union Events {
                tick: Tick
            }

            structure Tick {
                @eventHeader
                id: String
                choice: Choice
            }

            union Choice {
                doc: Document
            }
            """.asSmithyModel()

        val error = assertThrows<CodegenException> { clientIntegrationTest(model) }
        error.message!! shouldContain
            "has header members, so its body is read with the `protocol_serde` parser, but member `choice` can hold " +
            "a union with `DocumentV2` variants"
    }

    @Test
    fun `an initial response read by the protocol_serde parser can't reach an affected union`() {
        val model =
            """
            ${'$'}version: "2"
            namespace $SCHEMA_SERDE_NAMESPACE

            use aws.protocols#awsJson1_0

            @awsJson1_0
            service ChoiceService {
                version: "1"
                operations: [StreamChoices]
            }

            operation StreamChoices {
                output := {
                    events: Events
                    choice: Choice
                }
            }

            @streaming
            union Events {
                tick: Tick
            }

            structure Tick {
                value: String
            }

            union Choice {
                doc: Document
            }
            """.asSmithyModel()

        val error = assertThrows<CodegenException> { clientIntegrationTest(model) }
        error.message!! shouldContain
            "reads its initial response with the `protocol_serde` parser, but output member `choice` can hold a " +
            "union with `DocumentV2` variants"
    }

    companion object {
        private const val SCHEMA_SERDE_NAMESPACE = "smithy.rust.codegen.test.schemaheaders"
    }
}
