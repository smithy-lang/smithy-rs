/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule

/**
 * `DocumentV2` union variants on the shapes of Bedrock Agent Runtime's `InvokeFlow`: a sensitive input union, and
 * output event-stream events whose content unions hold documents.
 *
 * The SDK's model for that service isn't in `aws/sdk/aws-models`, so an SDK integration test wouldn't run in CI.
 * This model copies the operation's shapes from the `aws-sdk-rust` `release-2026-11-10` model: the same HTTP
 * bindings, traits, member order, and event-stream layout, without documentation and with fewer events, errors,
 * and enum values. The namespace is the one `SchemaSerdeAllowlist` reserves for tests.
 */
class DocumentV2InvokeFlowTest {
    private val model =
        """
        ${'$'}version: "2"
        namespace $SCHEMA_SERDE_NAMESPACE

        use aws.protocols#restJson1

        @restJson1
        service FlowService {
            version: "2023-07-26"
            operations: [InvokeFlow]
        }

        @http(code: 200, method: "POST", uri: "/flows/{flowIdentifier}/aliases/{flowAliasIdentifier}")
        operation InvokeFlow {
            input: InvokeFlowRequest
            output: InvokeFlowResponse
            errors: [ValidationException]
        }

        @input
        structure InvokeFlowRequest {
            @httpLabel
            @required
            flowIdentifier: String

            @httpLabel
            @required
            flowAliasIdentifier: String

            @required
            inputs: FlowInputs

            enableTrace: Boolean

            executionId: String
        }

        @length(min: 1, max: 1)
        list FlowInputs {
            member: FlowInput
        }

        structure FlowInput {
            @required
            nodeName: String

            nodeOutputName: String

            @required
            content: FlowInputContent

            nodeInputName: String
        }

        @sensitive
        union FlowInputContent {
            document: Document
        }

        @output
        structure InvokeFlowResponse {
            @httpPayload
            @required
            responseStream: FlowResponseStream

            @httpHeader("x-amz-bedrock-flow-execution-id")
            executionId: String
        }

        @streaming
        union FlowResponseStream {
            flowOutputEvent: FlowOutputEvent
            flowCompletionEvent: FlowCompletionEvent
            validationException: ValidationException
            flowMultiTurnInputRequestEvent: FlowMultiTurnInputRequestEvent
        }

        @sensitive
        structure FlowOutputEvent {
            @required
            nodeName: String

            @required
            nodeType: NodeType

            @required
            content: FlowOutputContent
        }

        union FlowOutputContent {
            document: Document
        }

        @sensitive
        structure FlowMultiTurnInputRequestEvent {
            @required
            nodeName: String

            @required
            nodeType: NodeType

            @required
            content: FlowMultiTurnInputContent
        }

        union FlowMultiTurnInputContent {
            document: Document
        }

        @sensitive
        structure FlowCompletionEvent {
            @required
            completionReason: FlowCompletionReason
        }

        enum FlowCompletionReason {
            SUCCESS
            INPUT_REQUIRED
        }

        enum NodeType {
            FLOW_INPUT_NODE = "FlowInputNode"
            FLOW_OUTPUT_NODE = "FlowOutputNode"
        }

        @error("client")
        @httpError(400)
        structure ValidationException {
            message: String
        }
        """.asSmithyModel()

    @Test
    fun `InvokeFlow sends and reads DocumentV2 union variants`() {
        // As the SDK does, keep `Exception` in error names, so the stream error is `ValidationException`.
        val sdkSettings =
            ObjectNode.builder()
                .withMember("codegen", ObjectNode.builder().withMember("renameErrors", false).build())
                .build()
        clientIntegrationTest(model, IntegrationTestParams(additionalSettings = sdkSettings)) { codegenContext, rustCrate ->
            check(SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)) {
                "this test needs DocumentV2 variants; check that $SCHEMA_SERDE_NAMESPACE is still in SchemaSerdeAllowlist"
            }
            val runtimeConfig = codegenContext.runtimeConfig
            rustCrate.testModule {
                rustTemplate(
                    """
                    use crate::config::DocumentUnionDecodeMode;
                    use crate::types::error::FlowResponseStreamError;
                    use crate::types::{
                        FlowCompletionReason, FlowInput, FlowInputContent, FlowMultiTurnInputContent,
                        FlowOutputContent, FlowResponseStream, NodeType,
                    };
                    use #{BigInteger};
                    use #{Document};
                    use #{DocumentObject};
                    use #{DocumentV2};
                    use std::str::FromStr;

                    const BIG: &str = "123456789012345678901234567890";

                    // An object whose keys aren't sorted and whose number doesn't fit in an `f64`.
                    fn object() -> DocumentV2 {
                        DocumentObject::from([
                            ("z".to_string(), DocumentV2::BigInteger(BigInteger::from_str(BIG).unwrap())),
                            ("a".to_string(), DocumentV2::from("text")),
                        ])
                        .into()
                    }

                    fn object_json() -> String {
                        format!(r##"{{"z":{BIG},"a":"text"}}"##)
                    }

                    fn keys(document: &DocumentV2) -> Vec<&str> {
                        document.as_object().unwrap().keys().map(String::as_str).collect()
                    }

                    fn input(content: FlowInputContent) -> FlowInput {
                        FlowInput::builder()
                            .node_name("FlowInputNode")
                            .node_output_name("document")
                            .content(content)
                            .build()
                            .unwrap()
                    }

                    async fn request_body(content: FlowInputContent) -> String {
                        let (http_client, request) = #{capture_request}(None);
                        let config = crate::Config::builder()
                            .endpoint_url("http://localhost:1234")
                            .http_client(http_client)
                            .behavior_version_latest()
                            .build();
                        let client = crate::Client::from_conf(config);
                        let _ = client
                            .invoke_flow()
                            .flow_identifier("flow")
                            .flow_alias_identifier("alias")
                            .inputs(input(content))
                            .send()
                            .await;
                        let request = request.expect_request();
                        assert_eq!(request.uri(), "http://localhost:1234/flows/flow/aliases/alias");
                        String::from_utf8(request.body().bytes().unwrap().to_vec()).unwrap()
                    }

                    #{TokioTest}
                    async fn the_request_sends_the_document_v2_input_exactly() {
                        let body = request_body(FlowInputContent::DocumentV2(object())).await;
                        assert_eq!(
                            body,
                            format!(
                                r##"{{"inputs":[{{"nodeName":"FlowInputNode","nodeOutputName":"document","content":{{"document":{}}}}}]}}"##,
                                object_json(),
                            ),
                        );

                        // The original variant is sent as before.
                        let body = request_body(FlowInputContent::Document(Document::from("text"))).await;
                        assert_eq!(
                            body,
                            r##"{"inputs":[{"nodeName":"FlowInputNode","nodeOutputName":"document","content":{"document":"text"}}]}"##,
                        );
                    }

                    ##[test]
                    fn the_sensitive_input_union_redacts_both_variants() {
                        for content in [FlowInputContent::DocumentV2(object()), FlowInputContent::Document(Document::from(BIG))] {
                            let debug = format!("{content:?} {:?}", input(content.clone()));
                            assert!(!debug.contains(BIG), "{debug}");
                        }
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

                    fn response_stream() -> #{http_1x}::Response<#{SdkBody}> {
                        let frames = [
                            frame(
                                "event",
                                ":event-type",
                                "flowOutputEvent",
                                &format!(
                                    r##"{{"nodeName":"FlowOutputNode","nodeType":"FlowOutputNode","content":{{"document":{}}}}}"##,
                                    object_json(),
                                ),
                            ),
                            frame(
                                "event",
                                ":event-type",
                                "flowMultiTurnInputRequestEvent",
                                r##"{"nodeName":"FlowInputNode","nodeType":"FlowInputNode","content":{"document":"What next?"}}"##,
                            ),
                            frame("event", ":event-type", "flowCompletionEvent", r##"{"completionReason":"INPUT_REQUIRED"}"##),
                            frame("exception", ":exception-type", "validationException", r##"{"message":"bad input"}"##),
                        ];
                        #{http_1x}::Response::builder()
                            .status(200)
                            .header("content-type", "application/vnd.amazon.eventstream")
                            .header("x-amz-bedrock-flow-execution-id", "execution")
                            .body(#{SdkBody}::from(frames.concat()))
                            .unwrap()
                    }

                    fn client(responses: usize) -> crate::Client {
                        let events = (0..responses)
                            .map(|_| {
                                #{ReplayEvent}::new(
                                    #{http_1x}::Request::builder()
                                        .uri("http://localhost:1234/")
                                        .body(#{SdkBody}::empty())
                                        .unwrap(),
                                    response_stream(),
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

                    // Reads the stream's events, and checks the events and the error that don't hold documents.
                    async fn read_stream(
                        mode: Option<DocumentUnionDecodeMode>,
                    ) -> (FlowOutputContent, FlowMultiTurnInputContent) {
                        let mut output = client(1)
                            .invoke_flow()
                            .flow_identifier("flow")
                            .flow_alias_identifier("alias")
                            .inputs(input(FlowInputContent::Document(Document::from("text"))))
                            .set_document_union_decode_mode(mode)
                            .send()
                            .await
                            .unwrap();
                        assert_eq!(output.execution_id(), Some("execution"));

                        let FlowResponseStream::FlowOutputEvent(event) = output.response_stream.recv().await.unwrap().unwrap() else {
                            panic!("expected a FlowOutputEvent");
                        };
                        assert_eq!(event.node_type, NodeType::FlowOutputNode);
                        let output_content = event.content;

                        let FlowResponseStream::FlowMultiTurnInputRequestEvent(event) =
                            output.response_stream.recv().await.unwrap().unwrap()
                        else {
                            panic!("expected a FlowMultiTurnInputRequestEvent");
                        };
                        let multi_turn_content = event.content;

                        let FlowResponseStream::FlowCompletionEvent(event) = output.response_stream.recv().await.unwrap().unwrap() else {
                            panic!("expected a FlowCompletionEvent");
                        };
                        assert_eq!(event.completion_reason, FlowCompletionReason::InputRequired);

                        let err = output.response_stream.recv().await.unwrap_err().into_service_error();
                        let FlowResponseStreamError::ValidationException(err) = err else {
                            panic!("expected ValidationException, got {err:?}");
                        };
                        assert_eq!(err.message(), Some("bad input"));
                        (output_content, multi_turn_content)
                    }

                    #{TokioTest}
                    async fn the_event_stream_reads_the_original_variants_by_default() {
                        let (output_content, multi_turn_content) = read_stream(None).await;
                        let FlowOutputContent::Document(document) = output_content else {
                            panic!("expected FlowOutputContent::Document, got {output_content:?}");
                        };
                        assert_eq!(document.as_object().unwrap()["a"], Document::from("text"));
                        assert_eq!(multi_turn_content, FlowMultiTurnInputContent::Document(Document::from("What next?")));

                        let (output_content, _) = read_stream(Some(DocumentUnionDecodeMode::V1)).await;
                        assert!(output_content.is_document());
                    }

                    #{TokioTest}
                    async fn the_event_stream_reads_document_v2_variants_in_v2_mode() {
                        let (output_content, multi_turn_content) = read_stream(Some(DocumentUnionDecodeMode::V2)).await;
                        let document = output_content.as_document_v2().unwrap();
                        assert_eq!(document, &object());
                        assert_eq!(keys(document), ["z", "a"]);
                        assert_eq!(multi_turn_content, FlowMultiTurnInputContent::DocumentV2(DocumentV2::from("What next?")));
                    }

                    #{TokioTest}
                    async fn a_document_v2_output_can_be_forwarded_exactly() {
                        let (output_content, _) = read_stream(Some(DocumentUnionDecodeMode::V2)).await;
                        let FlowOutputContent::DocumentV2(document) = output_content else {
                            panic!("expected FlowOutputContent::DocumentV2, got {output_content:?}");
                        };
                        let body = request_body(FlowInputContent::DocumentV2(document)).await;
                        assert!(body.contains(&format!(r##""content":{{"document":{}}}"##, object_json())), "{body}");
                    }
                    """,
                    "BigInteger" to RuntimeType.bigInteger(runtimeConfig),
                    "Document" to RuntimeType.document(runtimeConfig),
                    "DocumentObject" to RuntimeType.smithyTypes(runtimeConfig).resolve("document::DocumentObject"),
                    "DocumentV2" to RuntimeType.documentV2(runtimeConfig),
                    "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
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
                    // Renders `#[tokio::test]` and adds the `tokio` dev-dependency it needs.
                    "TokioTest" to writable { Attribute.TokioTest.render(this) },
                )
            }
        }
    }

    companion object {
        private const val SCHEMA_SERDE_NAMESPACE = "smithy.rust.codegen.test.schemaheaders"
    }
}
