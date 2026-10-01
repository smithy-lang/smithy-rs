/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.protocol.traits.Rpcv2CborTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customizations.SchemaSerdeAllowlist
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq

/**
 * The protocol swap matrix: for each protocol a client can be generated for, select every *other*
 * runtime protocol via `Config::builder().protocol(..)` and assert the whole request shape.
 *
 * This is the test the SEP asks for — "successfully make separate requests using the same version
 * of the implementation's SDK and same client (class), but with two different protocols"
 * (`.kiro/serialization-schema-decoupling.md`) — and it is the artifact that would have caught
 * https://github.com/smithy-lang/smithy-rs/issues/4801 and each of its siblings at once, rather
 * than one at a time.
 *
 * The invariant under test: **the request shape is a function of the selected protocol alone, not
 * of the protocol the client happened to be generated for.** Every expectation below is therefore
 * keyed only on the target protocol and asserted identically across all generated clients. A
 * framing header is asserted both present on the protocol that requires it and *absent* on the
 * protocols that do not, because the two failure directions are distinct: codegen omitting framing
 * on a swap-in, and codegen's framing persisting after a swap-out.
 *
 * Cost is kept down as the plan requires: one operation per model, request shape only, no server.
 * Each generated client is a single `clientIntegrationTest` hosting one test per target protocol.
 */
class ProtocolSwapMatrixTest {
    /**
     * A target protocol and the request shape it must produce, regardless of the client's generated
     * protocol.
     *
     * @param name used for the generated test function name.
     * @param construct the Rust expression selecting the protocol, as a `rustTemplate` fragment.
     * @param method expected HTTP method.
     * @param uri expected full URI, given an endpoint of `http://localhost:1234`.
     * @param contentType expected `Content-Type`.
     * @param framing framing headers that must be present, as name to value.
     */
    private data class Target(
        val name: String,
        val construct: String,
        val method: String,
        val uri: String,
        val contentType: String,
        val framing: List<Pair<String, String>> = emptyList(),
        /**
         * A response body encoding `value = "from-body"` in this protocol's wire format, as a Rust
         * expression. This is the response-side analogue of [contentType]: which codec reads the
         * body is a property of the selected protocol, so a body valid for one target is not valid
         * for another, and decoding it proves the right codec was chosen.
         */
        val responseBody: String = "",
    )

    /** Every framing header any target sets; each target asserts the ones it does not set are absent. */
    private val allFramingHeaders = listOf("smithy-protocol", "accept", "x-amz-target")

    /**
     * `SERVICE` and `OPERATION` are the shape names shared by every model below, so the
     * model-derived routes and target prefixes are the same string in every projection. That is
     * what makes one expectation table valid across all of them.
     */
    private val serviceName = "SwapMatrixService"

    private val targets =
        listOf(
            // Route derived from model facts in the config bag.
            Target(
                name = "rpcv2cbor",
                construct = "#{RpcV2CborProtocol}::new()",
                method = "POST",
                uri = "http://localhost:1234/service/$serviceName/operation/GetStats",
                contentType = "application/cbor",
                framing = listOf("smithy-protocol" to "rpc-v2-cbor", "accept" to "application/cbor"),
                // CBOR map(1) { text(5) "value": text(9) "from-body" }.
                responseBody =
                    """#{SdkBody}::from(
                    &b"\xa1\x65value\x69from-body"[..],
                    )""",
            ),
            // Fixed route; target prefix derived from the service shape name in the config bag.
            Target(
                name = "awsjson10",
                construct = "#{AwsJsonRpcProtocol}::aws_json_1_0()",
                method = "POST",
                uri = "http://localhost:1234/",
                contentType = "application/x-amz-json-1.0",
                framing = listOf("x-amz-target" to "$serviceName.GetStats"),
                responseBody = """#{SdkBody}::from("{\"value\":\"from-body\"}")""",
            ),
            // Fixed route; service version from the config bag, no framing headers at all.
            Target(
                name = "awsquery",
                construct = "#{AwsQueryProtocol}::new()",
                method = "POST",
                uri = "http://localhost:1234/",
                contentType = "application/x-www-form-urlencoded",
                // awsQuery strips the `<...Response><...Result>` envelope before decoding.
                responseBody =
                    """#{SdkBody}::from(
                    "<GetStatsResponse><GetStatsResult><value>from-body</value></GetStatsResult></GetStatsResponse>",
                    )""",
            ),
            // Route from the operation's `@http` trait, which is a property of the operation rather
            // than of the protocol, so here the endpoint codegen computed is authoritative.
            Target(
                name = "restjson1",
                construct = "#{AwsRestJsonProtocol}::new()",
                method = "PUT",
                uri = "http://localhost:1234/stats",
                contentType = "application/json",
                responseBody = """#{SdkBody}::from("{\"value\":\"from-body\"}")""",
            ),
            Target(
                name = "restxml",
                construct = "#{AwsRestXmlProtocol}::new()",
                method = "PUT",
                uri = "http://localhost:1234/stats",
                contentType = "application/xml",
                responseBody =
                    """#{SdkBody}::from("<GetStatsOutput><value>from-body</value></GetStatsOutput>")""",
            ),
        )

    /**
     * Target names whose selected protocol populates an output's `@httpHeader`,
     * `@httpPrefixHeaders` and `@httpResponseCode` members.
     *
     * This is the REST protocols only, as the SEP's codec-settings table requires: awsJson,
     * awsQuery, ec2Query and rpcv2Cbor all "ignore HTTP bindings", so for those the same members
     * must come from the protocol's own body representation or be absent.
     *
     * This set was every protocol until Phase 3 of
     * `.kiro/schema-serde-runtime-http-response-bindings-design.md` moved binding parsing out of
     * generated response code and into the protocol-owned composite. Generated code used to parse
     * headers and status unconditionally, before the selected protocol was consulted, so a member
     * bound to a header was populated no matter which protocol was selected. Narrowing this set is
     * what asserts that divergence is gone: with the old generated path these three rows fail with
     * `left: Some("from-header"), right: None`.
     */
    private val targetsReadingHttpBindings: Set<String> = setOf("restjson1", "restxml")

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
        )

    /**
     * A model carrying everything any target protocol needs: an `@http` trait so the REST targets
     * have a route to expand, and `@xmlNamespace` so restXml has a root namespace. The RPC targets
     * ignore both — an rpcv2Cbor client with an inert `@http` trait is exactly the #4801 shape.
     */
    private fun model(protocolAnnotation: String) =
        """
        namespace smithy.rust.codegen.test.schemaheaders

        @$protocolAnnotation
        @xmlNamespace(uri: "http://example.com/swap/")
        service $serviceName {
            version: "2024-01-01",
            operations: [GetStats]
        }

        @http(method: "PUT", uri: "/stats")
        operation GetStats {
            input := { name: String }
            output := {
                value: String

                @httpHeader("x-marker")
                marker: String

                @httpResponseCode
                code: Integer

                @httpPrefixHeaders("x-meta-")
                tags: TagMap
            }
        }

        map TagMap {
            key: String
            value: String
        }
        """.asSmithyModel(smithyVersion = "2.0")

    /**
     * Generates one client for [protocolAnnotation] and asserts every target protocol's request
     * shape against it.
     */
    private fun swapMatrixFor(
        protocolAnnotation: String,
        protocolId: ShapeId,
    ) {
        clientIntegrationTest(model(protocolAnnotation)) { context: ClientCodegenContext, rustCrate ->
            // A hard check rather than `assumeTrue`. This test gated itself on
            // `SchemaSerdeAllowlist.isProtocolEnabled(protocolId)` until the fixture moved to the
            // dedicated namespace, and because `allowedProtocols` is empty during rollout that gate
            // was false for every protocol — all four matrix tests silently skipped, so the central
            // architectural test of the SEP asserted nothing. The namespace-based mechanism exists
            // precisely so a fixture can exercise schema-exclusive generation independently of
            // production rollout state; a `check` makes a future regression in that wiring loud.
            check(SchemaSerdeAllowlist.usesSchemaSerdeExclusively(context)) {
                "the dedicated fixture namespace must exercise the schema-exclusive path, but " +
                    "$protocolId generated a client that is not schema-exclusive"
            }
            rustCrate.testModule {
                val scope = protocolScope(context.runtimeConfig)
                targets.forEach { target ->
                    tokioTest("swap_to_${target.name}") {
                        val framingAsserts =
                            target.framing.joinToString("\n") { (header, value) ->
                                """
                                assert_eq!(
                                    #{Some}(${value.dq()}),
                                    request.headers().get(${header.dq()}),
                                    "the selected protocol must set its own framing header $header",
                                );
                                """.trimIndent()
                            }
                        val absentAsserts =
                            allFramingHeaders.filterNot { name -> target.framing.any { it.first == name } }
                                .joinToString("\n") { header ->
                                    """
                                    assert_eq!(
                                        #{None},
                                        request.headers().get(${header.dq()}),
                                        "$header belongs to another protocol and must not survive the swap",
                                    );
                                    """.trimIndent()
                                }
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

                            let _ = client.get_stats().name("test").send().await;
                            let request = rx.expect_request();

                            assert_eq!(${target.method.dq()}, request.method());
                            assert_eq!(${target.uri.dq()}, request.uri());
                            assert_eq!(
                                #{Some}(${target.contentType.dq()}),
                                request.headers().get("Content-Type"),
                            );
                            $framingAsserts
                            $absentAsserts
                            """,
                            *RuntimeType.preludeScope,
                            *scope,
                            "capture_request" to RuntimeType.captureRequest(context.runtimeConfig),
                        )
                    }

                    tokioTest("swap_to_${target.name}_reads_its_own_response") {
                        val readsBindings = target.name in targetsReadingHttpBindings
                        val bindingAsserts =
                            if (readsBindings) {
                                """
                                assert_eq!(
                                    #{Some}("from-header"),
                                    output.marker(),
                                    "this protocol reads transport-bound members from the response",
                                );
                                assert_eq!(#{Some}(201), output.code());
                                assert_eq!(
                                    #{Some}(&"from-prefix".to_string()),
                                    output.tags().and_then(|m| m.get("k")),
                                    "a prefix-header map is a response binding like any other",
                                );
                                """.trimIndent()
                            } else {
                                """
                                assert_eq!(
                                    #{None},
                                    output.marker(),
                                    "this protocol ignores HTTP bindings, so a header-bound member \
                                     has no value in a body that does not carry it",
                                );
                                assert_eq!(#{None}, output.code());
                                assert_eq!(#{None}, output.tags().and_then(|m| m.get("k")));
                                """.trimIndent()
                            }
                        rustTemplate(
                            """
                            let respond = |_: #{http_1x}::Request<#{SdkBody}>| {
                                #{http_1x}::Response::builder()
                                    .status(201)
                                    .header("x-marker", "from-header")
                                    .header("x-meta-k", "from-prefix")
                                    .body(${target.responseBody})
                                    .unwrap()
                            };
                            let config = crate::Config::builder()
                                .http_client(#{infallible_client_fn}(respond))
                                .endpoint_url("http://localhost:1234")
                                .behavior_version_latest()
                                .protocol(${target.construct})
                                .build();
                            let client = crate::Client::from_conf(config);

                            let output = client
                                .get_stats()
                                .name("test")
                                .send()
                                .await
                                .expect("the selected protocol must decode a body in its own format");

                            // The decisive swap assertion: this body is valid for the selected
                            // protocol's codec and for no other, so decoding it proves the codec was
                            // chosen by the selected protocol rather than by the generated one.
                            assert_eq!(#{Some}("from-body"), output.value());
                            $bindingAsserts
                            """,
                            *RuntimeType.preludeScope,
                            *scope,
                            "SdkBody" to RuntimeType.sdkBody(context.runtimeConfig),
                            "http_1x" to CargoDependency.Http1x.toType(),
                            "infallible_client_fn" to
                                CargoDependency.smithyHttpClientTestUtil(context.runtimeConfig)
                                    .toType().resolve("test_util::infallible_client_fn"),
                        )
                    }
                }
            }
        }
    }

    @Test
    fun `rpcv2Cbor client honors every runtime-selected protocol`() =
        swapMatrixFor("smithy.protocols#rpcv2Cbor", Rpcv2CborTrait.ID)

    @Test
    fun `awsJson1_0 client honors every runtime-selected protocol`() =
        swapMatrixFor("aws.protocols#awsJson1_0", AwsJson1_0Trait.ID)

    @Test
    fun `restJson1 client honors every runtime-selected protocol`() =
        swapMatrixFor("aws.protocols#restJson1", RestJson1Trait.ID)

    @Test
    fun `restXml client honors every runtime-selected protocol`() =
        swapMatrixFor("aws.protocols#restXml", RestXmlTrait.ID)
}
