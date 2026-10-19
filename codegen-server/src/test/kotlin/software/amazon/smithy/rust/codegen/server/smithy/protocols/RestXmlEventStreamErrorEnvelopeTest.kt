/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

/**
 * A restXml server reads a modeled exception frame received in an input event stream from the
 * `@restXml` error envelope: `<ErrorResponse><Error>` by default, `<Error>` with
 * `noErrorWrapping`. The legacy generated parser establishes the expected outcomes, and the
 * schema server, which reads `noErrorWrapping` from its generated `ServiceSchema`, must match.
 */
internal class RestXmlEventStreamErrorEnvelopeTest {
    @ParameterizedTest(name = "noErrorWrapping: {0}")
    @ValueSource(booleans = [false, true])
    fun `exception frames use the legacy error envelope`(noErrorWrapping: Boolean) {
        val model =
            """
            ${"$"}version: "2"
            namespace test

            use aws.protocols#restXml

            @restXml(noErrorWrapping: $noErrorWrapping, http: ["h2", "http/1.1"], eventStreamHttp: ["h2"])
            service Svc { version: "1", operations: [Upload] }

            @http(method: "POST", uri: "/upload")
            operation Upload {
                input := { @httpPayload events: Events }
                output := {}
            }

            @streaming
            union Events { note: Note, failure: Failure }
            structure Note { text: String }

            @error("client")
            structure Failure { message: String }
            """.asSmithyModel(smithyVersion = "2")

        // (body, outcome): `message` for a decoded error, `none` for one with no message, `rejected`.
        val wrapped = if (noErrorWrapping) "rejected" else "failed"
        val bare = if (noErrorWrapping) "failed" else "rejected"
        val cases =
            listOf(
                "<ErrorResponse><Error><message>failed</message></Error></ErrorResponse>" to wrapped,
                "<ErrorResponse><RequestId>r</RequestId><Error><message>failed</message></Error></ErrorResponse>" to wrapped,
                "<Error><message>failed</message></Error>" to bare,
                "<Failure><message>failed</message></Failure>" to "rejected",
                "" to "none",
            ).joinToString(",\n") { (body, outcome) -> "(b${body.dqRaw()}.as_slice(), ${outcome.dqRaw()})" }

        for (schemaSerde in listOf(false, true)) {
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":$schemaSerde}}""").expectObjectNode(),
                ),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, rustCrate ->
                val server = ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType()
                val decode =
                    if (schemaSerde) {
                        """
                        use #{Server}::schema::event_stream::{DeserializableEventStream, EventFrame};
                        use #{Server}::schema::{MetadataRoutedProtocol, ProtocolBuildContext};
                        let protocol = #{Server}::schema::protocol::RestXml::from_build_context(
                            &ProtocolBuildContext::new(&crate::schema::service::SVC),
                        )
                        .unwrap();
                        let message = ::aws_smithy_types::event_stream::Message::new_from_parts(
                            vec![
                                ::aws_smithy_types::event_stream::Header::new(":message-type", ::aws_smithy_types::event_stream::HeaderValue::String("exception".into())),
                                ::aws_smithy_types::event_stream::Header::new(":exception-type", ::aws_smithy_types::event_stream::HeaderValue::String("failure".into())),
                                ::aws_smithy_types::event_stream::Header::new(":content-type", ::aws_smithy_types::event_stream::HeaderValue::String("application/xml".into())),
                            ],
                            body,
                        );
                        let frame = EventFrame::new(&message, protocol.event_stream_framing().unwrap());
                        match <crate::model::Events as DeserializableEventStream>::deserialize_error("failure", &frame) {
                            Ok(Some(crate::error::EventsError::Failure(failure))) => failure.message.unwrap_or_else(|| "none".to_owned()),
                            Ok(other) => panic!("unexpected {other:?}"),
                            Err(_) => "rejected".to_owned(),
                        }
                        """
                    } else {
                        """
                        match crate::protocol_serde::shape_failure::de_failure_xml_err(body, crate::error::Failure::builder()) {
                            Ok(builder) => builder.build().message.unwrap_or_else(|| "none".to_owned()),
                            Err(_) => "rejected".to_owned(),
                        }
                        """
                    }
                rustCrate.testModule {
                    unitTest("exception_frames_use_the_legacy_error_envelope") {
                        rustTemplate(
                            """
                            let decode = |body: &'static [u8]| -> #{String} { $decode };
                            for (body, expected) in [$cases] {
                                assert_eq!(decode(body), expected, "{}", #{String}::from_utf8_lossy(body));
                            }
                            """,
                            "Server" to server,
                            *RuntimeType.preludeScope,
                        )
                    }
                    if (schemaSerde) {
                        unitTest("the_service_schema_records_the_protocol_trait") {
                            rustTemplate(
                                """
                                let rest_xml = #{Server}::schema::traits::RestXmlTrait::from_service(&crate::schema::service::SVC).unwrap();
                                assert_eq!(rest_xml.no_error_wrapping(), $noErrorWrapping);
                                assert_eq!(rest_xml.http_configuration().http().unwrap(), ["h2", "http/1.1"]);
                                assert_eq!(rest_xml.http_configuration().event_stream_http().unwrap(), ["h2"]);
                                """,
                                "Server" to server,
                            )
                        }
                    }
                }
            }
        }
    }

    /** A Rust string literal for [this]; the bodies above contain no quotes or backslashes. */
    private fun String.dqRaw(): String = "\"$this\""
}
