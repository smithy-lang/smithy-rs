/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import org.junit.jupiter.api.Assumptions.assumeTrue
import org.junit.jupiter.api.Test
import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.protocol.traits.Rpcv2CborTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customizations.SchemaSerdeAllowlist
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
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
            ),
            // Fixed route; target prefix derived from the service shape name in the config bag.
            Target(
                name = "awsjson10",
                construct = "#{AwsJsonRpcProtocol}::aws_json_1_0()",
                method = "POST",
                uri = "http://localhost:1234/",
                contentType = "application/x-amz-json-1.0",
                framing = listOf("x-amz-target" to "$serviceName.GetStats"),
            ),
            // Fixed route; service version from the config bag, no framing headers at all.
            Target(
                name = "awsquery",
                construct = "#{AwsQueryProtocol}::new()",
                method = "POST",
                uri = "http://localhost:1234/",
                contentType = "application/x-www-form-urlencoded",
            ),
            // Route from the operation's `@http` trait, which is a property of the operation rather
            // than of the protocol, so here the endpoint codegen computed is authoritative.
            Target(
                name = "restjson1",
                construct = "#{AwsRestJsonProtocol}::new()",
                method = "PUT",
                uri = "http://localhost:1234/stats",
                contentType = "application/json",
            ),
            Target(
                name = "restxml",
                construct = "#{AwsRestXmlProtocol}::new()",
                method = "PUT",
                uri = "http://localhost:1234/stats",
                contentType = "application/xml",
            ),
        )

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
        namespace test

        @$protocolAnnotation
        @xmlNamespace(uri: "http://example.com/swap/")
        service $serviceName {
            version: "2024-01-01",
            operations: [GetStats]
        }

        @http(method: "PUT", uri: "/stats")
        operation GetStats {
            input := { name: String }
            output := { value: String }
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
        assumeTrue(
            SchemaSerdeAllowlist.isProtocolEnabled(protocolId),
            "$protocolId is not on SchemaSerdeAllowlist, so the schema-serde request path is not generated",
        )
        clientIntegrationTest(model(protocolAnnotation)) { context: ClientCodegenContext, rustCrate ->
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
