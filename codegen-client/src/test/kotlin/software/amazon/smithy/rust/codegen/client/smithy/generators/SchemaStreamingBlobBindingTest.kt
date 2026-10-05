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
 * Streaming blob outputs on the schema-serde path are constructed builder-first: the selected
 * protocol's response deserializer populates the output builder from headers and status while
 * omitting the streaming payload, every borrow of the response is then dropped, the live body is
 * moved onto the builder, and only then is the output finalized.
 *
 * Three properties are asserted end to end:
 *
 * - header and status bindings are populated, and the live body is delivered intact rather than
 *   being read by the body codec or replaced by an empty stream;
 * - finalization runs the canonical required-member correction shared with the other response
 *   paths. The previous build-then-set-the-stream path finalized inside the shape's `deserialize`,
 *   whose defaults cover strings but not enums, so a missing `@required` enum failed the build; and
 * - the stream is set after member population, so a `@required` streaming payload does not fail the
 *   build.
 */
class SchemaStreamingBlobBindingTest {
    private val model =
        """
        namespace smithy.rust.codegen.test.schemaheaders

        use aws.protocols#restJson1

        @restJson1
        service StreamingBindingService {
            version: "2023-01-01",
            operations: [GetBlob],
        }

        @http(uri: "/blob", method: "GET")
        @readonly
        operation GetBlob {
            output: GetBlobOutput,
        }

        @streaming
        blob StreamingBlob

        structure GetBlobOutput {
            @required
            @httpPayload
            body: StreamingBlob,

            @httpHeader("x-marker")
            marker: String,

            @httpResponseCode
            code: Integer,

            @required
            @httpHeader("x-required")
            required: String,

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
        """.asSmithyModel(smithyVersion = "2.0")

    @Test
    fun `a streaming blob output reads its bindings and keeps the live body`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            check(SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)) {
                "this test must exercise the schema-exclusive streaming blob path"
            }
            val scope =
                arrayOf(
                    *RuntimeType.preludeScope,
                    "SdkBody" to RuntimeType.sdkBody(codegenContext.runtimeConfig),
                    "http_1x" to CargoDependency.Http1x.toType(),
                    "infallible_client_fn" to
                        CargoDependency.smithyHttpClientTestUtil(codegenContext.runtimeConfig)
                            .toType().resolve("test_util::infallible_client_fn"),
                )
            rustCrate.testModule {
                tokioTest("bindings_and_the_live_body_are_both_delivered") {
                    rustTemplate(
                        """
                        let http_client = #{infallible_client_fn}(|_req| {
                            #{http_1x}::Response::builder()
                                .status(200)
                                .header("x-marker", "from-header")
                                .header("x-required", "present")
                                .header("x-state", "READY")
                                .header("x-meta-kind", "from-prefix")
                                // Not a valid document in any codec: if the body codec were asked
                                // to read it, the call would fail rather than stream it.
                                .body(#{SdkBody}::from("{not json"))
                                .unwrap()
                        });
                        let client = crate::Client::from_conf(
                            crate::Config::builder()
                                .http_client(http_client)
                                .endpoint_url("http://localhost:1234")
                                .build(),
                        );

                        let output = client.get_blob().send().await.expect("a 200 streaming response");
                        assert_eq!(#{Some}("from-header"), output.marker());
                        assert_eq!(#{Some}(200), output.code());
                        assert_eq!("present", output.required());
                        assert_eq!(&crate::types::State::Ready, output.state());
                        assert_eq!(
                            #{Some}(&"from-prefix".to_string()),
                            output.tags().expect("the prefix map is always populated").get("kind"),
                        );
                        let bytes = output.body.collect().await.expect("the body streams").into_bytes();
                        assert_eq!(&b"{not json"[..], &bytes[..]);
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
                                .body(#{SdkBody}::from("payload"))
                                .unwrap()
                        });
                        let client = crate::Client::from_conf(
                            crate::Config::builder()
                                .http_client(http_client)
                                .endpoint_url("http://localhost:1234")
                                .build(),
                        );

                        // `required` and `state` are absent from the wire. Finalization applies the
                        // canonical required-member correction instead of failing the build. The enum is
                        // what discriminates: the shape's own `deserialize` has no default for an enum,
                        // so the previous build-then-set-the-stream path failed here with MissingField.
                        let output = client.get_blob().send().await.expect("a 200 streaming response");
                        assert_eq!("", output.required());
                        assert_eq!("no value was set", output.state().as_str());
                        assert_eq!(#{None}, output.marker());
                        let bytes = output.body.collect().await.expect("the body streams").into_bytes();
                        assert_eq!(&b"payload"[..], &bytes[..]);
                        """,
                        *scope,
                    )
                }
            }
        }
    }
}
