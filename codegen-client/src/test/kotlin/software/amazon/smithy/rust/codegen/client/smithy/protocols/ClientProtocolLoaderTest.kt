/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import org.junit.jupiter.api.extension.ExtensionContext
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.Arguments
import org.junit.jupiter.params.provider.ArgumentsProvider
import org.junit.jupiter.params.provider.ArgumentsSource
import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.aws.traits.protocols.AwsJson1_1Trait
import software.amazon.smithy.aws.traits.protocols.AwsQueryTrait
import software.amazon.smithy.aws.traits.protocols.Ec2QueryTrait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.protocol.traits.Rpcv2CborTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationGenerator
import software.amazon.smithy.rust.codegen.client.smithy.protocols.ClientProtocolLoader.Companion.DefaultProtocols
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolMap
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import java.util.stream.Stream

data class TestCase(
    val supportedProtocols: ProtocolMap<OperationGenerator, ClientCodegenContext>,
    val model: Model,
    val resolvedProtocol: String?,
)

class ClientProtocolLoaderTest {
    @Test
    fun `test priority order of default supported protocols`() {
        val expectedOrder =
            listOf(
                Rpcv2CborTrait.ID,
                AwsJson1_0Trait.ID,
                AwsJson1_1Trait.ID,
                RestJson1Trait.ID,
                RestXmlTrait.ID,
                AwsQueryTrait.ID,
                Ec2QueryTrait.ID,
            )
        assertEquals(expectedOrder, DefaultProtocols.keys.toList())
    }

    // Although the test function name appears generic, its purpose is to verify whether
    // the RPCv2Cbor protocol is selected based on specific contexts.
    @ParameterizedTest
    @ArgumentsSource(ProtocolSelectionTestCaseProvider::class)
    fun `should resolve expected protocol`(testCase: TestCase) {
        val protocolLoader = ClientProtocolLoader(testCase.supportedProtocols)
        val serviceShape = testCase.model.expectShape(ShapeId.from("test#TestService"), ServiceShape::class.java)
        if (testCase.resolvedProtocol.isNullOrEmpty()) {
            assertThrows<CodegenException> {
                protocolLoader.protocolFor(testCase.model, serviceShape)
            }
        } else {
            val actual = protocolLoader.protocolFor(testCase.model, serviceShape).first.name
            assertEquals(testCase.resolvedProtocol, actual)
        }
    }
}

class ProtocolSelectionTestCaseProvider : ArgumentsProvider {
    override fun provideArguments(p0: ExtensionContext?): Stream<out Arguments> {
        val protocolsWithoutRpcv2Cbor = LinkedHashMap(DefaultProtocols)
        protocolsWithoutRpcv2Cbor.remove(Rpcv2CborTrait.ID)

        return arrayOf(
            TestCase(DefaultProtocols, model(listOf("rpcv2Cbor", "awsJson1_0")), "rpcv2Cbor"),
            TestCase(DefaultProtocols, model(listOf("rpcv2Cbor")), "rpcv2Cbor"),
            TestCase(DefaultProtocols, model(listOf("rpcv2Cbor", "awsJson1_0", "awsQuery")), "rpcv2Cbor"),
            TestCase(DefaultProtocols, model(listOf("awsJson1_0", "awsQuery")), "awsJson1_0"),
            TestCase(DefaultProtocols, model(listOf("awsQuery")), "awsQuery"),
            TestCase(protocolsWithoutRpcv2Cbor, model(listOf("rpcv2Cbor", "awsJson1_0")), "awsJson1_0"),
            TestCase(protocolsWithoutRpcv2Cbor, model(listOf("rpcv2Cbor")), null),
            TestCase(protocolsWithoutRpcv2Cbor, model(listOf("rpcv2Cbor", "awsJson1_0", "awsQuery")), "awsJson1_0"),
            TestCase(protocolsWithoutRpcv2Cbor, model(listOf("awsJson1_0", "awsQuery")), "awsJson1_0"),
            TestCase(protocolsWithoutRpcv2Cbor, model(listOf("awsQuery")), "awsQuery"),
        ).map { Arguments.of(it) }.stream()
    }

    private fun model(protocols: List<String>) =
        (
            """
            namespace test
            """ + renderProtocols(protocols) +
                """
                @xmlNamespace(uri: "http://test.com") // required for @awsQuery
                service TestService {
                    version: "1.0.0"
                }
                """
        ).asSmithyModel(smithyVersion = "2.0")
}

private fun renderProtocols(protocols: List<String>): String {
    val (rpcProtocols, awsProtocols) = protocols.partition { it == "rpcv2Cbor" }

    val uses =
        buildList {
            rpcProtocols.forEach { add("use smithy.protocols#$it") }
            awsProtocols.forEach { add("use aws.protocols#$it") }
        }.joinToString("\n")

    val annotations = protocols.joinToString("\n") { "@$it" }

    return """
        $uses

        $annotations
    """
}
