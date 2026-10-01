/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.customizations.SchemaSerdeAllowlist
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest

/**
 * Event-stream outputs on the schema-serde path now receive their modeled response headers and
 * status. Previously that path set the event receiver on the builder and finalized it without
 * consulting the protocol at all, so every header- or status-bound member of an event-stream
 * output was silently absent.
 *
 * The generated path swaps the live body out first, installs the receiver, and only then asks the
 * selected protocol for a response deserializer over the now-bodyless response. Three properties
 * are asserted:
 *
 * - a REST protocol populates headers, status, and prefix headers, and the receiver still works;
 * - a missing `@required` binding is corrected at finalization rather than failing the build. An
 *   enum is used because it is the member type whose correction differs between finalizers; and
 * - a body-only protocol selected at runtime ignores those bindings, and the empty body left
 *   behind by the swap is not an error for it.
 */
class SchemaEventStreamBindingTest {
    private val model =
        """
        namespace smithy.rust.codegen.test.schemaheaders

        use aws.protocols#restJson1

        @restJson1
        service EventStreamBindingService {
            version: "2023-01-01",
            operations: [Subscribe],
        }

        @http(uri: "/subscribe", method: "POST")
        operation Subscribe {
            output: SubscribeOutput,
        }

        structure SubscribeOutput {
            @httpPayload
            events: Events,

            @httpHeader("x-marker")
            marker: String,

            @httpResponseCode
            code: Integer,

            @required
            @httpHeader("x-state")
            state: State,

            @httpPrefixHeaders("x-meta-")
            tags: TagMap,
        }

        enum State {
            READY = "READY"
            DONE = "DONE"
        }

        map TagMap {
            key: String,
            value: String
        }

        @streaming
        union Events {
            tick: Tick,
        }

        structure Tick {
            value: String,
        }
        """.asSmithyModel(smithyVersion = "2.0")

    @Test
    fun `an event stream output reads its response bindings through the selected protocol`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            check(SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)) {
                "this test must exercise the schema-exclusive event-stream path"
            }
            val rc = codegenContext.runtimeConfig
            val scope =
                arrayOf(
                    *RuntimeType.preludeScope,
                    "SdkBody" to RuntimeType.sdkBody(rc),
                    "http_1x" to CargoDependency.Http1x.toType(),
                    "infallible_client_fn" to
                        CargoDependency.smithyHttpClientTestUtil(rc).toType().resolve("test_util::infallible_client_fn"),
                    "AwsJsonRpcProtocol" to
                        RuntimeType.smithyJson(rc).resolve("protocol::aws_json_rpc::AwsJsonRpcProtocol"),
                )
            rustCrate.testModule {
                tokioTest("a_rest_protocol_populates_headers_status_and_prefix_headers") {
                    rustTemplate(
                        """
                        let http_client = #{infallible_client_fn}(|_req| {
                            #{http_1x}::Response::builder()
                                .status(200)
                                .header("x-marker", "from-header")
                                .header("x-state", "READY")
                                .header("x-meta-kind", "from-prefix")
                                .header("content-type", "application/vnd.amazon.eventstream")
                                .body(#{SdkBody}::empty())
                                .unwrap()
                        });
                        let client = crate::Client::from_conf(
                            crate::Config::builder()
                                .http_client(http_client)
                                .endpoint_url("http://localhost:1234")
                                .build(),
                        );

                        let mut output = client.subscribe().send().await.expect("a 200 event-stream response");
                        assert_eq!(#{Some}("from-header"), output.marker());
                        assert_eq!(#{Some}(200), output.code());
                        assert_eq!(&crate::types::State::Ready, output.state());
                        assert_eq!(
                            #{Some}(&"from-prefix".to_string()),
                            output.tags().expect("the prefix map is always populated").get("kind"),
                        );
                        // The receiver installed before member population is still the live one:
                        // an empty body is a clean end of stream, not an error.
                        assert!(output.events.recv().await.expect("a clean end of stream").is_none());
                        """,
                        *scope,
                    )
                }

                tokioTest("a_missing_required_binding_is_corrected_at_finalization") {
                    rustTemplate(
                        """
                        let http_client = #{infallible_client_fn}(|_req| {
                            #{http_1x}::Response::builder()
                                .status(200)
                                .header("content-type", "application/vnd.amazon.eventstream")
                                .body(#{SdkBody}::empty())
                                .unwrap()
                        });
                        let client = crate::Client::from_conf(
                            crate::Config::builder()
                                .http_client(http_client)
                                .endpoint_url("http://localhost:1234")
                                .build(),
                        );

                        let output = client.subscribe().send().await.expect("a 200 event-stream response");
                        assert_eq!("no value was set", output.state().as_str());
                        assert_eq!(#{None}, output.marker());
                        """,
                        *scope,
                    )
                }

                tokioTest("a_body_only_protocol_selected_at_runtime_ignores_the_bindings") {
                    rustTemplate(
                        """
                        let http_client = #{infallible_client_fn}(|_req| {
                            #{http_1x}::Response::builder()
                                .status(200)
                                .header("x-marker", "from-header")
                                .header("x-state", "READY")
                                .header("x-meta-kind", "from-prefix")
                                .header("content-type", "application/vnd.amazon.eventstream")
                                .body(#{SdkBody}::empty())
                                .unwrap()
                        });
                        let client = crate::Client::from_conf(
                            crate::Config::builder()
                                .http_client(http_client)
                                .endpoint_url("http://localhost:1234")
                                .protocol(#{AwsJsonRpcProtocol}::aws_json_1_0())
                                .build(),
                        );

                        let output = client.subscribe().send().await
                            .expect("an empty body left by the stream swap is not an error for a body-only protocol");
                        assert_eq!(#{None}, output.marker(), "awsJson1_0 ignores @httpHeader");
                        assert_eq!(#{None}, output.code(), "awsJson1_0 ignores @httpResponseCode");
                        assert_eq!("no value was set", output.state().as_str());
                        """,
                        *scope,
                    )
                }
            }
        }
    }
}
