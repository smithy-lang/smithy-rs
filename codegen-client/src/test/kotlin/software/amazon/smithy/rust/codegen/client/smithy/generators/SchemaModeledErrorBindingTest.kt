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
 * Modeled errors on the schema-serde path are constructed builder-first: the selected protocol
 * returns an error-mode response deserializer, the error's own protocol-agnostic
 * `deserialize_members` consumer populates a builder from it, error metadata and the fallback
 * message are applied to that builder, and only then is the error finalized.
 *
 * Two properties need end-to-end coverage that nothing else in the repository provides.
 *
 * First, HTTP response bindings on a modeled error are now supplied by the runtime protocol rather
 * than by generated parsing inside the error type's response method. The benchmark clients do model
 * errors with `@httpHeader` members, but their generated cases are timing harnesses that discard the
 * parse result (`let _ = black_box(parsed)`), so they assert nothing about correctness.
 *
 * Second, the generated response method's `if body.is_empty() { return ... }` short circuit no longer
 * runs on this path, so an error response with no body is handled entirely by the error-mode
 * composite. Note that this is defensive rather than observable through a built-in protocol pairing:
 * restJson1 resolves the error code from `x-amzn-errortype`, so an empty body still reaches a modeled
 * variant, but its JSON codec already treats empty input as an empty object; restXml's codec does
 * reject empty input, but it resolves the error code from the body envelope, so an empty body never
 * selects a modeled variant in the first place. The runtime unit tests in
 * `aws-smithy-schema`'s `http_protocol::response` cover the skip directly with a codec that rejects
 * empty input. What this test pins down is that the generated path keeps working without the guard.
 */
class SchemaModeledErrorBindingTest {
    private val model =
        """
        namespace smithy.rust.codegen.test.schemaheaders

        use aws.protocols#restJson1

        @restJson1
        service ErrorBindingService {
            version: "2023-01-01",
            operations: [GetThing],
        }

        @http(uri: "/thing", method: "GET")
        operation GetThing {
            output: GetThingOutput,
            errors: [DetailedError],
        }

        structure GetThingOutput {
            name: String,
        }

        @error("client")
        @httpError(400)
        structure DetailedError {
            message: String,

            @httpHeader("x-detail")
            detail: String,

            @httpPrefixHeaders("x-meta-")
            tags: TagMap,
        }

        map TagMap {
            key: String,
            value: String
        }
        """.asSmithyModel(smithyVersion = "2.0")

    @Test
    fun `a modeled error is populated from both its response bindings and its body`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            check(SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)) {
                "this test must exercise the schema-exclusive modeled-error path"
            }
            rustCrate.testModule {
                tokioTest("a_modeled_error_reads_headers_prefix_and_body") {
                    rustTemplate(
                        """
                        let http_client = #{infallible_client_fn}(|_req| {
                            #{http_1x}::Response::builder()
                                .status(400)
                                .header("x-amzn-errortype", "DetailedError")
                                .header("x-detail", "from-header")
                                .header("x-meta-kind", "from-prefix")
                                .body(#{SdkBody}::from(r##"{"message":"from-body"}"##))
                                .unwrap()
                        });
                        let client = crate::Client::from_conf(
                            crate::Config::builder()
                                .http_client(http_client)
                                .endpoint_url("http://localhost:1234")
                                .build(),
                        );

                        let err = client.get_thing().send().await.expect_err("the response is a 400");
                        let err = err.into_service_error();
                        let err = match err {
                            crate::operation::get_thing::GetThingError::DetailedError(e) => e,
                            other => panic!("expected the modeled variant, got {other:?}"),
                        };

                        // Body member, read by the protocol's payload codec.
                        assert_eq!(#{Some}("from-body"), err.message());
                        // Header member, read by the runtime response composite.
                        assert_eq!(#{Some}("from-header"), err.detail());
                        // Prefix-header map, likewise. An absent prefix yields an empty map, so
                        // asserting the entry rather than `is_some` keeps the assertion honest.
                        assert_eq!(
                            #{Some}(&"from-prefix".to_string()),
                            err.tags().expect("the prefix map is always populated").get("kind"),
                        );
                        // Error metadata survives finalization through the builder.
                        assert_eq!(
                            #{Some}("DetailedError"),
                            #{ProvideErrorMetadata}::meta(&err).code(),
                        );
                        """,
                        *RuntimeType.preludeScope,
                        "SdkBody" to RuntimeType.sdkBody(codegenContext.runtimeConfig),
                        "http_1x" to CargoDependency.Http1x.toType(),
                        "ProvideErrorMetadata" to
                            RuntimeType.smithyTypes(codegenContext.runtimeConfig)
                                .resolve("error::metadata::ProvideErrorMetadata"),
                        "infallible_client_fn" to
                            CargoDependency.smithyHttpClientTestUtil(codegenContext.runtimeConfig)
                                .toType().resolve("test_util::infallible_client_fn"),
                    )
                }

                tokioTest("a_modeled_error_with_no_body_still_reads_its_bindings_and_falls_back_to_the_generic_message") {
                    rustTemplate(
                        """
                        let http_client = #{infallible_client_fn}(|_req| {
                            #{http_1x}::Response::builder()
                                .status(400)
                                .header("x-amzn-errortype", "DetailedError")
                                .header("x-detail", "from-header")
                                .body(#{SdkBody}::empty())
                                .unwrap()
                        });
                        let client = crate::Client::from_conf(
                            crate::Config::builder()
                                .http_client(http_client)
                                .endpoint_url("http://localhost:1234")
                                .build(),
                        );

                        let err = client.get_thing().send().await.expect_err("the response is a 400");
                        let err = err.into_service_error();
                        let err = match err {
                            crate::operation::get_thing::GetThingError::DetailedError(e) => e,
                            other => panic!("expected the modeled variant, got {other:?}"),
                        };

                        assert_eq!(#{Some}("from-header"), err.detail());
                        // No body means no modeled message, and the envelope carried none either,
                        // so the fallback applied to the builder leaves it absent rather than
                        // failing the build.
                        assert_eq!(#{None}, err.message());
                        // An empty body must not be mistaken for "no prefix headers were modeled".
                        assert!(err.tags().expect("the prefix map is always populated").is_empty());
                        """,
                        *RuntimeType.preludeScope,
                        "SdkBody" to RuntimeType.sdkBody(codegenContext.runtimeConfig),
                        "http_1x" to CargoDependency.Http1x.toType(),
                        "infallible_client_fn" to
                            CargoDependency.smithyHttpClientTestUtil(codegenContext.runtimeConfig)
                                .toType().resolve("test_util::infallible_client_fn"),
                    )
                }
            }
        }
    }
}
