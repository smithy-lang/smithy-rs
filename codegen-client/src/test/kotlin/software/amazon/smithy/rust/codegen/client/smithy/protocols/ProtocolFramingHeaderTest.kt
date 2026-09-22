/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import io.kotest.matchers.collections.shouldContain
import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertAll
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.smithy.protocols.ClientProtocolLoader.Companion.DefaultProtocols
import software.amazon.smithy.rust.codegen.client.testutil.testClientCodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

/**
 * Guards the classification that the schema-serde request path depends on.
 *
 * `Protocol` splits additional request headers into [Protocol.protocolFramingHeaders] — which the
 * schema path deliberately drops, because the runtime `ClientProtocol` sets them inside
 * `serialize_request` so they stay correct across `Config::builder().protocol(..)` — and
 * [Protocol.serviceRequestHeaders], which both codegen paths emit. Nothing about the type system
 * enforces which bucket a given header belongs in, so a misclassified framing header would be
 * emitted by codegen and silently survive a protocol swap. That is precisely the shape of
 * https://github.com/smithy-lang/smithy-rs/issues/4801.
 *
 * These tests are driven off [DefaultProtocols] rather than a hand-written list so that a newly
 * added protocol is covered without anyone remembering to extend them. The previous
 * name-based skip in `RequestSerializerGenerator` did not grow when rpcv2Cbor added two framing
 * headers, which is how that gap survived.
 */
class ProtocolFramingHeaderTest {
    /**
     * Headers that are a function of the protocol alone. A protocol that reports one of these as a
     * *service* header would have codegen emit it, defeating the split.
     *
     * `content-type` and `content-length` are included because they are payload concerns owned by
     * the request-serializer branches, never by the additional-headers hooks.
     */
    private val framingHeaderNames =
        setOf(
            "smithy-protocol",
            "accept",
            "x-amz-target",
            "content-type",
            "content-length",
        )

    @Test
    fun `no protocol classifies a framing header as a service header`() {
        assertAll(
            eachProtocol().map { (protocolId, protocol, operation) ->
                {
                    val misclassified =
                        protocol.serviceRequestHeaders(operation).map { it.first }
                            .filter { it.lowercase() in framingHeaderNames }
                    withClue(protocolId, "serviceRequestHeaders leaked protocol framing $misclassified") {
                        misclassified shouldBe emptyList()
                    }
                }
            },
        )
    }

    /**
     * A protocol that overrides `additionalRequestHeaders` directly, instead of one of the two
     * halves, would have its header dropped by the schema path with no other signal. Asserting the
     * derived relationship still holds catches that.
     */
    @Test
    fun `additionalRequestHeaders is exactly framing plus service headers`() {
        assertAll(
            eachProtocol().map { (protocolId, protocol, operation) ->
                {
                    withClue(protocolId, "additionalRequestHeaders diverged from its two halves") {
                        protocol.additionalRequestHeaders(operation) shouldBe
                            protocol.protocolFramingHeaders(operation) + protocol.serviceRequestHeaders(operation)
                    }
                }
            },
        )
    }

    /**
     * The positive half of the split: `x-amzn-query-mode` comes from `@awsQueryCompatible` on the
     * *service*, not from the protocol — the same service sends it under either awsJson or
     * rpcv2Cbor — so it must stay a service header and keep being emitted by codegen. Classifying
     * it as framing would drop it from every `@awsQueryCompatible` client.
     */
    @Test
    fun `x-amzn-query-mode is a service header, not protocol framing`() {
        val model =
            """
            namespace test

            @aws.protocols#awsJson1_0
            @aws.protocols#awsQueryCompatible
            @aws.api#service(sdkId: "Test")
            service TestService {
                version: "1.0.0",
                operations: [SomeOperation]
            }

            operation SomeOperation {
                input := { name: String }
                output := { name: String }
            }
            """.asSmithyModel(smithyVersion = "2.0").let(OperationNormalizer::transform)
        val context = testClientCodegenContext(model)
        val protocol = DefaultProtocols[ShapeId.from("aws.protocols#awsJson1_0")]!!.protocol(context)
        val operation = model.expectShape(ShapeId.from("test#SomeOperation"), OperationShape::class.java)

        protocol.serviceRequestHeaders(operation).map { it.first } shouldContain "x-amzn-query-mode"
        protocol.protocolFramingHeaders(operation).map { it.first } shouldContain "x-amz-target"
    }

    /** Every protocol in the loader's default map, instantiated against a model that uses it. */
    private fun eachProtocol(): List<Triple<ShapeId, Protocol, OperationShape>> =
        DefaultProtocols.map { (protocolId, factory) ->
            val model =
                """
                namespace test

                @$protocolId
                @aws.api#service(sdkId: "Test")
                @xmlNamespace(uri: "http://test.com")
                service TestService {
                    version: "1.0.0",
                    operations: [SomeOperation]
                }

                @http(method: "PUT", uri: "/op")
                operation SomeOperation {
                    input := { name: String }
                    output := { name: String }
                }
                """.asSmithyModel(smithyVersion = "2.0").let(OperationNormalizer::transform)
            Triple(
                protocolId,
                factory.protocol(testClientCodegenContext(model)),
                model.expectShape(ShapeId.from("test#SomeOperation"), OperationShape::class.java),
            )
        }

    private fun <T> withClue(
        protocolId: ShapeId,
        message: String,
        block: () -> T,
    ): T =
        try {
            block()
        } catch (e: AssertionError) {
            throw AssertionError("$protocolId: $message\n${e.message}", e)
        }
}
