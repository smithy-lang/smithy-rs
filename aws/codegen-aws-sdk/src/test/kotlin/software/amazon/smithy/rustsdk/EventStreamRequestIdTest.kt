/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest

/**
 * Regression test for the schema-serde event-stream response deserializer
 * populating `_request_id` from the `x-amzn-requestid` response header.
 *
 * restJson1 is on [SchemaSerdeAllowlist] (see
 * `codegen-client/.../customizations/SchemaDecorator.kt`), so the generated
 * client hits `deserializeStreamingEventStreamSchema` in
 * [ResponseDeserializerGenerator] rather than the legacy streaming path.
 *
 * Before the fix (PR #4628), the schema-serde event-stream path skipped the
 * `MutateOutput` customization emitted by [BaseRequestIdDecorator], so
 * `output.request_id()` always returned `None` even when the service set
 * `x-amzn-requestid`.
 */
class EventStreamRequestIdTest {
    companion object {
        // `$version` is escaped to avoid Kotlin interpolation in a raw-string.
        private const val PREFIX = "\$version: \"2\""
        val model =
            """
            $PREFIX
            namespace test

            use aws.api#service
            use aws.auth#sigv4
            use aws.protocols#restJson1
            use smithy.rules#endpointRuleSet

            @service(sdkId: "dontcare")
            @restJson1
            @sigv4(name: "dontcare")
            @auth([sigv4])
            @endpointRuleSet({
                "version": "1.0",
                "rules": [{ "type": "endpoint", "conditions": [], "endpoint": { "url": "https://example.com" } }],
                "parameters": {
                    "Region": { "required": false, "type": "String", "builtIn": "AWS::Region" },
                }
            })
            service TestService {
                version: "2023-01-01",
                operations: [StreamOperation]
            }

            @http(uri: "/StreamOperation", method: "POST")
            @optionalAuth
            operation StreamOperation {
                input: StreamInput,
                output: StreamOutput,
            }

            @input
            structure StreamInput {
                @httpPayload
                body: StreamBody,
            }

            @output
            structure StreamOutput {
                @httpPayload
                events: TestStream,
            }

            @streaming
            union TestStream {
                hello: HelloEvent,
            }

            // Unused by this test but required to make the input valid — every
            // `@streaming` union must appear in an operation input so the
            // marshaller generator runs.
            @streaming
            union StreamBody {
                ping: PingEvent,
            }

            structure HelloEvent {
                @eventPayload
                data: String,
            }

            structure PingEvent {}
            """.asSmithyModel()
    }

    @Test
    fun `request_id is populated on event stream responses`() {
        awsSdkIntegrationTest(model) { context, rustCrate ->
            rustCrate.integrationTest("event_stream_request_id") {
                tokioTest("request_id_is_set_from_x_amzn_requestid_header") {
                    val rc = context.runtimeConfig
                    val moduleName = context.moduleUseName()
                    rustTemplate(
                        """
                        use $moduleName::config::{Credentials, Region};
                        use $moduleName::{Config, Client};
                        // Re-exported by BaseRequestIdDecorator.extras.
                        use $moduleName::operation::RequestId;

                        // An empty event-stream body is a valid zero-frame stream.
                        // The deserializer still constructs the output and sets
                        // headers before wrapping the body in an EventReceiver.
                        let http_client = #{StaticReplayClient}::new(vec![
                            #{ReplayEvent}::new(
                                #{HttpRequest1x}::builder()
                                    .uri("https://example.com/StreamOperation")
                                    .body(#{SdkBody}::empty())
                                    .unwrap(),
                                #{HttpResponse1x}::builder()
                                    .status(200)
                                    .header("x-amzn-requestid", "test-request-id-1234")
                                    .header("content-type", "application/vnd.amazon.eventstream")
                                    .body(#{SdkBody}::empty())
                                    .unwrap(),
                            ),
                        ]);

                        let config = Config::builder()
                            .credentials_provider(Credentials::for_tests())
                            .region(Region::new("us-east-1"))
                            .http_client(http_client.clone())
                            .build();
                        let client = Client::from_conf(config);

                        // We don't need to send any input events for this test —
                        // the deserializer produces the output as soon as headers
                        // arrive. We pass an empty stream for the `body` input.
                        let input_stream = #{futures_util}::stream::empty();
                        let output = client
                            .stream_operation()
                            .body(input_stream.into())
                            .send()
                            .await
                            .expect("request should succeed");

                        assert_eq!(
                            Some("test-request-id-1234"),
                            output.request_id(),
                            "request_id should be populated from x-amzn-requestid header",
                        );
                        """,
                        "HttpRequest1x" to RuntimeType.HttpRequest1x,
                        "HttpResponse1x" to RuntimeType.HttpResponse1x,
                        "ReplayEvent" to
                            RuntimeType.smithyHttpClientTestUtil(rc)
                                .resolve("test_util::ReplayEvent"),
                        "SdkBody" to RuntimeType.sdkBody(rc),
                        "StaticReplayClient" to
                            RuntimeType.smithyHttpClientTestUtil(rc)
                                .resolve("test_util::StaticReplayClient"),
                        "futures_util" to CargoDependency.FuturesUtil.toType(),
                    )
                }
            }
        }
    }
}
