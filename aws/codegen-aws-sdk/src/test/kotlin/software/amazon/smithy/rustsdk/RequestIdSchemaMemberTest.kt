/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.customizations.SchemaSerdeAllowlist
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest

/**
 * Covers the AWS request ID now that it is no longer a synthetic schema member.
 *
 * [BaseRequestIdDecorator] used to append a synthetic `requestId` member carrying
 * `@httpHeader("x-amzn-requestid")` to every output's schema. That had three problems, and this
 * test pins the resolution of each:
 *
 *  1. The request ID is metadata about a response, not a modeled shape fact, so a schema member
 *     was the wrong representation. `MutateOutput` is now the sole writer on every generated
 *     path.
 *  2. The member set a response-binding mask bit on every AWS output, so no AWS output could ever
 *     take the runtime's body-only fast path.
 *  3. Most seriously, the runtime composite reads schema members as *modeled* headers, so it
 *     applied `NonUtf8HeaderHandling` to them. An unreadable `x-amzn-requestid` would have failed
 *     the whole response under the default reject policy, where `impl RequestId for Headers` uses
 *     the UTF-8 string accessor and simply yields `None`. That was not reachable in a shipped SDK
 *     — the only allowlisted production namespace is awsJson1_1, which never builds the composite
 *     — but it would have become live for the first REST AWS service on the schema path.
 *
 * The model deliberately also has a *modeled* `requestId` body member. Its Rust name is
 * `request_id`, which is exactly the schema member name the synthetic member used to carry, so this
 * is the case where the two representations came closest to each other. They must stay independent:
 * the modeled member is read from the body and reached through the inherent accessor, while the
 * metadata is reached through the `RequestId` trait.
 *
 * This is an AWS codegen test rather than a `SchemaGeneratorTest` case because the synthetic field,
 * its accessor trait, and the `MutateOutput` customization all come from [BaseRequestIdDecorator].
 *
 * The mutation that justifies these assertions restores all three of the removed sites — the member
 * schema static, the reference from the schema's member array, and the consumer arm. Restoring only
 * the first two is not sufficient: with no consumer arm the composite routes the member but nothing
 * reads it, so no parse and no rejection happens. With all three restored, the unreadable-header
 * test fails with `ResponseError(... header was not valid utf-8)`, which is the hazard in point 3
 * above, measured rather than argued.
 */
class RequestIdSchemaMemberTest {
    companion object {
        // `$version` is escaped to avoid Kotlin interpolation in a raw string.
        private const val PREFIX = "\$version: \"2\""

        // The namespace is allowlisted for schema-exclusive generation, so this test does not
        // depend on protocol rollout state. Using `isProtocolEnabled` here would silently skip.
        val model =
            """
            $PREFIX
            namespace smithy.rust.codegen.test.schemaheaders

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
                operations: [GetThing]
            }

            @http(uri: "/GetThing", method: "GET")
            @optionalAuth
            operation GetThing {
                input: GetThingInput,
                output: GetThingOutput,
            }

            @input
            structure GetThingInput {}

            @output
            structure GetThingOutput {
                /// A modeled body member whose Rust name, `request_id`, is the schema member name
                /// the synthetic member used to carry. It must stay an ordinary body member.
                requestId: String,

                @httpHeader("x-marker")
                marker: String,
            }
            """.asSmithyModel()
    }

    @Test
    fun `the request id is response metadata rather than a schema member`() {
        awsSdkIntegrationTest(model) { context, rustCrate ->
            check(SchemaSerdeAllowlist.usesSchemaSerdeExclusively(context)) {
                "this test must exercise schema-exclusive generation"
            }
            val rc = context.runtimeConfig
            val moduleName = context.moduleUseName()
            rustCrate.integrationTest("request_id_schema_member") {
                rustTemplate(
                    """
                    use $moduleName::config::{Credentials, Region};
                    use $moduleName::{Config, Client};
                    // Re-exported by BaseRequestIdDecorator.extras.
                    use $moduleName::operation::RequestId;

                    fn client(http_client: impl #{HttpConnector} + 'static) -> Client {
                        Client::from_conf(
                            Config::builder()
                                .behavior_version_latest()
                                .credentials_provider(Credentials::for_tests())
                                .region(Region::new("us-east-1"))
                                .http_client(http_client)
                                .build(),
                        )
                    }
                    """,
                    "HttpConnector" to RuntimeType.smithyRuntimeApiClient(rc).resolve("client::http::HttpClient"),
                )

                // The structural half. A response binding on an output's schema is exactly what
                // defeats the body-only fast path, so assert the schema carries none rather than
                // only asserting the behavior it would have produced.
                rustTemplate(
                    """
                    ##[test]
                    fn no_schema_member_is_bound_to_the_request_id_header() {
                        let members = $moduleName::operation::get_thing::GetThingOutput::SCHEMA.members();
                        // Exactly the two modeled members, in model order. The synthetic member was
                        // appended after them, so a third name here means it is back in the schema.
                        let names: Vec<_> = members.iter().map(|m| m.member_name().unwrap()).collect();
                        assert_eq!(vec!["requestId", "marker"], names);
                        // `requestId` is an ordinary body member: no response binding at all.
                        assert!(
                            members[0].http_header().is_none(),
                            "the modeled requestId member must not be header bound",
                        );
                        // And `marker` is the only response-bound member, so an AWS output with no
                        // modeled bindings now has an empty binding mask.
                        assert!(members[1].http_header().is_some());
                    }
                    """,
                )

                tokioTest("the_request_id_comes_from_the_header_and_the_modeled_member_from_the_body") {
                    rustTemplate(
                        """
                        let http_client = #{infallible_client_fn}(|_: #{http_1x}::Request<#{SdkBody}>| {
                            #{http_1x}::Response::builder()
                                .status(200)
                                .header("x-amzn-requestid", "hdr-request-id")
                                .header("x-marker", "from-header")
                                .body(#{SdkBody}::from(r##"{"requestId":"from-body"}"##))
                                .unwrap()
                        });
                        let output = client(http_client).get_thing().send().await.expect("should succeed");

                        // Response metadata, written by BaseRequestIdDecorator's MutateOutput.
                        assert_eq!(Some("hdr-request-id"), RequestId::request_id(&output));
                        // The modeled body member is untouched by it.
                        assert_eq!(Some("from-body"), output.request_id());
                        assert_eq!(Some("from-header"), output.marker());
                        """,
                        "SdkBody" to RuntimeType.sdkBody(rc),
                        "http_1x" to CargoDependency.Http1x.toType(),
                        "infallible_client_fn" to
                            CargoDependency.smithyHttpClientTestUtil(rc).toType()
                                .resolve("test_util::infallible_client_fn"),
                    )
                }

                tokioTest("an_unreadable_request_id_header_does_not_fail_the_response") {
                    rustTemplate(
                        """
                        // The modeled `x-marker` header is readable, so the only unreadable value
                        // belongs to the unmodeled request-ID header. Under the default reject
                        // policy a *modeled* member with this value would fail the response; an
                        // unmodeled one must not.
                        let http_client = #{infallible_client_fn}(|_: #{http_1x}::Request<#{SdkBody}>| {
                            #{http_1x}::Response::builder()
                                .status(200)
                                .header(
                                    "x-amzn-requestid",
                                    #{http_1x}::HeaderValue::from_bytes(b"id-\xe9").unwrap(),
                                )
                                .header("x-marker", "from-header")
                                .body(#{SdkBody}::from(r##"{}"##))
                                .unwrap()
                        });
                        let output = client(http_client)
                            .get_thing()
                            .send()
                            .await
                            .expect("an unreadable unmodeled header must not fail the response");

                        // Best effort: the UTF-8 accessor yields nothing for this value.
                        assert_eq!(None, RequestId::request_id(&output));
                        // The modeled binding still worked.
                        assert_eq!(Some("from-header"), output.marker());
                        """,
                        "SdkBody" to RuntimeType.sdkBody(rc),
                        "http_1x" to CargoDependency.Http1x.toType(),
                        "infallible_client_fn" to
                            CargoDependency.smithyHttpClientTestUtil(rc).toType()
                                .resolve("test_util::infallible_client_fn"),
                    )
                }
            }
        }
    }
}
