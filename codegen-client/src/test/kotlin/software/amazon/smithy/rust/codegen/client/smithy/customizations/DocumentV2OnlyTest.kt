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
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustSettings
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.traits.DocumentV2OnlyTrait
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import java.nio.file.Files
import kotlin.io.path.extension
import kotlin.io.path.readText

/**
 * Members marked with [DocumentV2OnlyTrait] next to members that keep the side-by-side `Document` and `DocumentV2`
 * API, in the same structures, unions, and operations.
 *
 * Members named `old*` stand for members published before `DocumentV2` and keep both forms. Members named `new*`, and
 * the members of the `NewOp`, `PutPayload`, and `StreamThings` operations, stand for members added later and use
 * `DocumentV2` only.
 */
class DocumentV2OnlyTest {
    private val model =
        """
        ${'$'}version: "2"
        namespace $SCHEMA_SERDE_NAMESPACE

        use aws.protocols#restJson1
        use smithy.test#httpRequestTests
        use smithy.test#httpResponseTests

        @restJson1
        service MixedService {
            version: "1"
            operations: [PutThing, NewOp, PutPayload, StreamThings]
        }

        @http(uri: "/thing", method: "POST")
        @httpRequestTests([
            {
                id: "PutThingWritesDocumentV2OnlyMembers",
                protocol: restJson1,
                method: "POST",
                uri: "/thing",
                headers: { "Content-Type": "application/json" },
                body: "{\"oldPayload\":\"old\",\"newPayload\":{\"a\":1},\"newList\":[true,\"x\"],\"newMap\":{\"k\":\"v\"},\"newRequired\":\"r\"}",
                bodyMediaType: "application/json",
                params: {
                    oldPayload: "old",
                    newPayload: { a: 1 },
                    newList: [true, "x"],
                    newMap: { k: "v" },
                    newRequired: "r"
                },
                appliesTo: "client"
            }
        ])
        @httpResponseTests([
            {
                id: "PutThingReadsDocumentV2OnlyMembers",
                protocol: restJson1,
                code: 200,
                headers: { "Content-Type": "application/json" },
                body: "{\"oldPayload\":\"old\",\"newPayload\":{\"a\":1}}",
                bodyMediaType: "application/json",
                params: {
                    oldPayload: "old",
                    newPayload: { a: 1 }
                },
                appliesTo: "client"
            }
        ])
        operation PutThing {
            input := {
                oldPayload: Document
                newPayload: Document
                newList: DocumentList
                newMap: DocumentMap
                newSparse: SparseDocumentList
                @required
                newRequired: Document
                shared: Shared
                choice: Choice
            }
            output := {
                oldPayload: Document
                newPayload: Document
                shared: Shared
                choice: Choice
            }
            errors: [ThingError]
        }

        // A new operation that reuses a shared structure with both kinds of members.
        @http(uri: "/new", method: "POST")
        operation NewOp {
            input := {
                payload: Document
                shared: Shared
            }
            output := {
                payload: Document
                choice: NewOnlyChoice
            }
        }

        @http(uri: "/payload", method: "POST")
        operation PutPayload {
            input := {
                @httpPayload
                body: Document
            }
            output := {
                @httpPayload
                body: Document
            }
        }

        @http(uri: "/stream", method: "POST")
        operation StreamThings {
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
            payload: Document
        }

        structure Shared {
            oldDoc: Document
            newDoc: Document
            @required
            newRequired: Document
            @default("fallback")
            newDefaulted: Document
        }

        union Choice {
            oldJson: Document
            newJson: Document
            newList: DocumentList
            text: String
        }

        union NewOnlyChoice {
            json: Document
            text: String
        }

        @error("client")
        structure ThingError {
            message: String
            oldDetails: Document
            newDetails: Document
        }

        list DocumentList {
            member: Document
        }

        map DocumentMap {
            key: String
            value: Document
        }

        @sparse
        list SparseDocumentList {
            member: Document
        }
        """.asSmithyModel()

    private val markNewMembers =
        MarkDocumentV2Only { model ->
            listOf("newPayload", "newList", "newMap", "newSparse", "newRequired").map {
                model.inputMember("PutThing", it)
            } +
                listOf(
                    model.outputMember("PutThing", "newPayload"),
                    model.inputMember("NewOp", "payload"),
                    model.outputMember("NewOp", "payload"),
                    model.inputMember("PutPayload", "body"),
                    model.outputMember("PutPayload", "body"),
                ) +
                listOf(
                    "Shared\$newDoc",
                    "Shared\$newRequired",
                    "Shared\$newDefaulted",
                    "Choice\$newJson",
                    "Choice\$newList",
                    "NewOnlyChoice\$json",
                    "ThingError\$newDetails",
                    "Tick\$payload",
                ).map { ShapeId.from("$SCHEMA_SERDE_NAMESPACE#$it") }
        }

    @Test
    fun `members that use DocumentV2 only have one representation next to members with both`() {
        val crateDir =
            clientIntegrationTest(model, additionalDecorators = listOf(markNewMembers)) { codegenContext, rustCrate ->
                val runtimeConfig = codegenContext.runtimeConfig
                val smithySchema = RuntimeType.smithySchema(runtimeConfig)
                rustCrate.testModule {
                    rustTemplate(
                        """
                        use crate::config::DocumentUnionDecodeMode;
                        use crate::operation::put_thing::{PutThingInput, PutThingOutput};
                        use crate::types::error::ThingError;
                        use crate::types::{Choice, NewOnlyChoice, Shared};
                        use #{Codec};
                        use #{SerializableStruct};
                        use #{ShapeDeserializer};
                        use #{ShapeSerializer};
                        use #{BigInteger};
                        use #{Document};
                        use #{DocumentV2};
                        use std::str::FromStr;

                        const BIG: &str = "123456789012345678901234567890";

                        // JSON writes this value as a number, and `Document` would hold it as the nearest `f64`.
                        fn big() -> DocumentV2 {
                            DocumentV2::BigInteger(BigInteger::from_str(BIG).unwrap())
                        }

                        fn v1(value: &str) -> Document {
                            Document::from(value)
                        }

                        fn v2(value: &str) -> DocumentV2 {
                            DocumentV2::from(value)
                        }

                        fn json(value: &dyn SerializableStruct) -> String {
                            let mut ser = #{JsonCodec}::default().create_serializer();
                            ser.write_struct(value.schema(), value).unwrap();
                            String::from_utf8(ser.finish()).unwrap()
                        }

                        fn deser(json: &str) -> impl ShapeDeserializer + '_ {
                            #{JsonCodec}::default().create_deserializer(json.as_bytes())
                        }

                        fn response(status: u16, headers: &[(&str, &str)], body: impl Into<#{SdkBody}>) -> #{http_1x}::Response<#{SdkBody}> {
                            let mut builder = #{http_1x}::Response::builder().status(status);
                            for (name, value) in headers {
                                builder = builder.header(*name, *value);
                            }
                            builder.body(body.into()).unwrap()
                        }

                        fn config(http_client: impl #{HttpClient} + 'static) -> crate::Config {
                            crate::Config::builder()
                                .endpoint_url("http://localhost:1234")
                                .http_client(http_client)
                                .behavior_version_latest()
                                .build()
                        }

                        ##[test]
                        fn members_that_use_document_v2_only_hold_it_in_their_normal_api() {
                            let input = PutThingInput::builder()
                                .old_payload(v1("old"))
                                .new_payload(big())
                                .new_list(big())
                                .new_list(v2("x"))
                                .new_map("k", big())
                                .set_new_sparse(Some(vec![Some(big()), None]))
                                .new_required(v2("r"))
                                .build()
                                .unwrap();
                            let new_payload: &Option<DocumentV2> = &input.new_payload;
                            assert_eq!(new_payload, &Some(big()));
                            assert_eq!(input.new_payload(), Some(&big()));
                            assert_eq!(input.new_list, Some(vec![big(), v2("x")]));
                            assert_eq!(input.new_map.as_ref().unwrap()["k"], big());
                            assert_eq!(input.new_sparse, Some(vec![Some(big()), None]));
                            assert_eq!(input.new_required, Some(v2("r")));

                            // A member with both forms keeps them.
                            let old_payload: &Option<Document> = &input.old_payload;
                            assert_eq!(old_payload, &Some(v1("old")));
                            assert_eq!(input.old_payload_v2(), None);
                        }

                        ##[test]
                        fn a_shared_structure_holds_both_kinds_of_members() {
                            let shared = Shared::builder()
                                .old_doc(v1("old"))
                                .new_doc(big())
                                .new_required(v2("r"))
                                .build()
                                .unwrap();
                            assert_eq!(shared.old_doc, Some(v1("old")));
                            assert_eq!(shared.new_doc, Some(big()));
                            let required: &DocumentV2 = &shared.new_required;
                            assert_eq!(required, &v2("r"));
                            // The modeled default holds `DocumentV2` too.
                            let defaulted: &DocumentV2 = &shared.new_defaulted;
                            assert_eq!(defaulted, &v2("fallback"));

                            let error = Shared::builder().old_doc(v1("old")).build().unwrap_err();
                            assert!(error.to_string().contains("new_required"), "{error}");

                            let debug = format!("{shared:?}");
                            assert!(debug.contains(r##"old_doc: Some(String("old"))"##), "{debug}");
                            assert!(debug.contains(r##"new_required: String("r")"##), "{debug}");
                        }

                        ##[test]
                        fn union_members_that_use_document_v2_only_have_one_variant() {
                            let choice = Choice::NewJson(big());
                            assert_eq!(choice.as_new_json(), Ok(&big()));
                            assert!(choice.is_new_json());
                            assert_eq!(Choice::NewList(vec![big()]).as_new_list(), Ok(&vec![big()]));
                            assert_eq!(NewOnlyChoice::Json(big()).as_json(), Ok(&big()));

                            // A member with both forms keeps its additive variant.
                            assert!(!Choice::OldJson(v1("x")).is_old_json_v2());
                            assert_eq!(Choice::OldJsonV2(big()).as_old_json_v2(), Ok(&big()));
                        }

                        ##[test]
                        fn schema_serde_writes_members_that_use_document_v2_only_exactly() {
                            let input = PutThingInput::builder()
                                .new_payload(big())
                                .new_list(big())
                                .new_map("k", big())
                                .set_new_sparse(Some(vec![Some(big()), None]))
                                .choice(Choice::NewJson(big()))
                                .build()
                                .unwrap();
                            assert_eq!(
                                json(&input),
                                format!(
                                    r##"{{"newPayload":{BIG},"newList":[{BIG}],"newMap":{{"k":{BIG}}},"newSparse":[{BIG},null],"choice":{{"newJson":{BIG}}}}}"##
                                ),
                            );

                            let input = PutThingInput::builder()
                                .choice(Choice::NewList(vec![big()]))
                                .build()
                                .unwrap();
                            assert_eq!(json(&input), format!(r##"{{"choice":{{"newList":[{BIG}]}}}}"##));
                        }

                        ##[test]
                        fn schema_serde_reads_members_that_use_document_v2_only_exactly() {
                            let payload = format!(r##"{{"z":{BIG},"a":0.1234567890123456789}}"##);
                            let text = format!(r##"{{"oldPayload":{BIG},"newPayload":{payload}}}"##);
                            let output = PutThingOutput::deserialize(&mut deser(&text)).unwrap();
                            let new_payload = output.new_payload.as_ref().unwrap().as_object().unwrap();
                            assert_eq!(new_payload.keys().map(String::as_str).collect::<Vec<_>>(), ["z", "a"]);
                            assert_eq!(new_payload["z"], big());
                            // The member with both forms reads into its companion.
                            assert_eq!(output.old_payload_v2(), Some(&big()));

                            // A value read is written back unchanged.
                            let text = format!(r##"{{"newPayload":{payload}}}"##);
                            let input = PutThingInput::deserialize(&mut deser(&text)).unwrap();
                            assert_eq!(json(&input), text);

                            let error = ThingError::deserialize(&mut deser(&format!(r##"{{"newDetails":{BIG}}}"##))).unwrap();
                            assert_eq!(error.new_details, Some(big()));
                        }

                        ##[test]
                        fn union_members_that_use_document_v2_only_read_it_in_either_mode() {
                            let read = |mode: DocumentUnionDecodeMode, text: String| {
                                mode.read_with(&mut deser(&text), |d| PutThingOutput::deserialize(d))
                                    .unwrap()
                                    .choice
                                    .unwrap()
                            };
                            for mode in [DocumentUnionDecodeMode::V1, DocumentUnionDecodeMode::V2] {
                                assert_eq!(read(mode, format!(r##"{{"choice":{{"newJson":{BIG}}}}}"##)), Choice::NewJson(big()));
                                assert_eq!(read(mode, format!(r##"{{"choice":{{"newList":[{BIG}]}}}}"##)), Choice::NewList(vec![big()]));
                            }

                            // The member with both forms still follows the mode.
                            let old = format!(r##"{{"choice":{{"oldJson":{BIG}}}}}"##);
                            assert!(read(DocumentUnionDecodeMode::V1, old.clone()).is_old_json());
                            assert_eq!(read(DocumentUnionDecodeMode::V2, old), Choice::OldJsonV2(big()));
                        }

                        #{TokioTest}
                        async fn the_client_sends_members_that_use_document_v2_only() {
                            let (http_client, request) = #{capture_request}(None);
                            let client = crate::Client::from_conf(config(http_client));
                            let _ = client.put_thing().new_payload(big()).new_required(v2("r")).send().await;
                            let request = request.expect_request();
                            let body = std::str::from_utf8(request.body().bytes().unwrap()).unwrap();
                            assert_eq!(body, format!(r##"{{"newPayload":{BIG},"newRequired":"r"}}"##));
                        }

                        #{TokioTest}
                        async fn an_http_payload_document_uses_document_v2() {
                            let (http_client, request) = #{capture_request}(Some(response(
                                200,
                                &[("content-type", "application/json")],
                                BIG,
                            )));
                            let client = crate::Client::from_conf(config(http_client));
                            let output = client.put_payload().body(big()).send().await.unwrap();
                            let request = request.expect_request();
                            assert_eq!(std::str::from_utf8(request.body().bytes().unwrap()).unwrap(), BIG);
                            assert_eq!(output.body, Some(big()));
                        }

                        #{TokioTest}
                        async fn events_read_members_that_use_document_v2_only() {
                            let message = #{Message}::new_from_parts(
                                vec![
                                    #{Header}::new(":message-type", #{HeaderValue}::String("event".into())),
                                    #{Header}::new(":event-type", #{HeaderValue}::String("tick".into())),
                                    #{Header}::new(":content-type", #{HeaderValue}::String("application/json".into())),
                                ],
                                format!(r##"{{"payload":{BIG}}}"##),
                            );
                            let mut frame = Vec::new();
                            #{write_message_to}(&message, &mut frame).unwrap();
                            let stream = response(200, &[("content-type", "application/vnd.amazon.eventstream")], frame);
                            let replay = #{StaticReplayClient}::new(vec![#{ReplayEvent}::new(
                                #{http_1x}::Request::builder()
                                    .uri("http://localhost:1234/")
                                    .body(#{SdkBody}::empty())
                                    .unwrap(),
                                stream,
                            )]);
                            let client = crate::Client::from_conf(config(replay));
                            let mut output = client.stream_things().send().await.unwrap();
                            let event = output.events.recv().await.unwrap().unwrap();
                            assert_eq!(event.as_tick().unwrap().payload, Some(big()));
                        }
                        """,
                        "Codec" to smithySchema.resolve("codec::Codec"),
                        "SerializableStruct" to smithySchema.resolve("serde::SerializableStruct"),
                        "ShapeDeserializer" to smithySchema.resolve("serde::ShapeDeserializer"),
                        "ShapeSerializer" to smithySchema.resolve("serde::ShapeSerializer"),
                        "JsonCodec" to RuntimeType.smithyJson(runtimeConfig).resolve("codec::JsonCodec"),
                        "BigInteger" to RuntimeType.bigInteger(runtimeConfig),
                        "Document" to RuntimeType.document(runtimeConfig),
                        "DocumentV2" to RuntimeType.documentV2(runtimeConfig),
                        "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
                        "HttpClient" to
                            RuntimeType.smithyRuntimeApiClient(runtimeConfig).resolve("client::http::HttpClient"),
                        "http_1x" to CargoDependency.Http1x.toType(),
                        "capture_request" to RuntimeType.captureRequest(runtimeConfig),
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
                        "TokioTest" to writable { Attribute.TokioTest.render(this) },
                    )
                }
            }

        val sources = { path: String ->
            Files.walk(crateDir.resolve(path)).use { paths ->
                paths.filter { it.extension == "rs" }.map { it.readText() }.toList()
            }.joinToString("\n")
        }
        val allSources = sources("src")

        // Members that use `DocumentV2` only get no `_v2` methods or additive variants.
        for (name in listOf("new_payload_v2", "new_list_v2", "new_doc_v2", "new_details_v2", "body_v2", "NewJsonV2")) {
            allSources shouldNotContain name
        }
        crateDir.resolve("src/types/_new_only_choice.rs").readText() shouldNotContain "V2("
        allSources shouldContain "old_payload_v2"
        allSources shouldContain "OldJsonV2("

        // The protocol tests convert their `Document` values.
        allSources shouldContain "document_v2_slot::into_document_v2("

        // Only an operation whose response reaches a member with both union variants gets the decode mode.
        sources("src/operation/put_thing") shouldContain "pub fn document_union_decode_mode("
        sources("src/operation/new_op") shouldNotContain "document_union_decode_mode"

        // A structure whose document members all use `DocumentV2` only keeps its `Debug` derive.
        allSources shouldContain Regex("""derive\([^)]*::std::fmt::Debug[^)]*\)]\s*pub struct NewOpOutput """)
    }

    @Test
    fun `a client without schema serde can't have members that use DocumentV2 only`() {
        val model =
            """
            namespace com.example

            use aws.protocols#awsJson1_0

            @awsJson1_0
            service FooService {
                version: "1"
                operations: [Foo]
            }

            operation Foo {
                input := {
                    payload: Document
                }
            }
            """.asSmithyModel(smithyVersion = "2.0")

        val error =
            assertThrows<CodegenException> {
                clientIntegrationTest(
                    model,
                    additionalDecorators =
                        listOf(
                            MarkDocumentV2Only { listOf(it.inputMember("Foo", "payload", namespace = "com.example")) },
                        ),
                )
            }
        error.message!! shouldContain "`com.example#FooService` doesn't use schema serde, but member"
        error.message!! shouldContain "uses `DocumentV2` only, which only schema serde can read and write"
    }

    @Test
    fun `an event body read by the protocol_serde parser can't hold a member that uses DocumentV2 only`() {
        val model =
            """
            ${'$'}version: "2"
            namespace $SCHEMA_SERDE_NAMESPACE

            use aws.protocols#restJson1

            @restJson1
            service StreamService {
                version: "1"
                operations: [StreamThings]
            }

            @http(uri: "/stream", method: "POST")
            operation StreamThings {
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
                payload: Document
            }
            """.asSmithyModel()

        val error =
            assertThrows<CodegenException> {
                clientIntegrationTest(
                    model,
                    additionalDecorators =
                        listOf(MarkDocumentV2Only { listOf(ShapeId.from("$SCHEMA_SERDE_NAMESPACE#Tick\$payload")) }),
                )
            }
        error.message!! shouldContain
            "has header members, so its body is read with the `protocol_serde` parser, but member `payload` is or " +
            "holds a member that uses `DocumentV2` only"
    }

    @Test
    fun `an initial response read by the protocol_serde parser can't hold a member that uses DocumentV2 only`() {
        val model =
            """
            ${'$'}version: "2"
            namespace $SCHEMA_SERDE_NAMESPACE

            use aws.protocols#awsJson1_0

            @awsJson1_0
            service StreamService {
                version: "1"
                operations: [StreamThings]
            }

            operation StreamThings {
                output := {
                    events: Events
                    payload: Document
                }
            }

            @streaming
            union Events {
                tick: Tick
            }

            structure Tick {
                value: String
            }
            """.asSmithyModel()

        val error =
            assertThrows<CodegenException> {
                clientIntegrationTest(
                    model,
                    additionalDecorators =
                        listOf(MarkDocumentV2Only { listOf(it.outputMember("StreamThings", "payload")) }),
                )
            }
        error.message!! shouldContain
            "reads its initial response with the `protocol_serde` parser, but output member `payload` is or holds a " +
            "member that uses `DocumentV2` only"
    }

    /**
     * Marks the members that [select] returns with [DocumentV2OnlyTrait], after the client's model transforms have
     * created the operations' input and output structures.
     */
    private class MarkDocumentV2Only(private val select: (Model) -> List<ShapeId>) : ClientCodegenDecorator {
        override val name: String = "MarkDocumentV2Only"
        override val order: Byte = 0

        override fun classpathDiscoverable(): Boolean = false

        override fun transformModel(
            service: ServiceShape,
            model: Model,
            settings: ClientRustSettings,
        ): Model = DocumentV2OnlyTrait.markMembers(model, select(model))
    }

    companion object {
        private const val SCHEMA_SERDE_NAMESPACE = "smithy.rust.codegen.test.schemaheaders"

        private fun Model.operation(
            name: String,
            namespace: String,
        ) = expectShape(ShapeId.fromParts(namespace, name), OperationShape::class.java)

        private fun Model.inputMember(
            operation: String,
            member: String,
            namespace: String = SCHEMA_SERDE_NAMESPACE,
        ): ShapeId = operation(operation, namespace).inputShape.withMember(member)

        private fun Model.outputMember(
            operation: String,
            member: String,
            namespace: String = SCHEMA_SERDE_NAMESPACE,
        ): ShapeId = operation(operation, namespace).outputShape.withMember(member)
    }
}
