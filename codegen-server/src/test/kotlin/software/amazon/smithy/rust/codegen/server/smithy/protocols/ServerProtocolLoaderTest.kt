/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import io.kotest.assertions.throwables.shouldThrow
import io.kotest.matchers.shouldBe
import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.aws.traits.protocols.AwsJson1_1Trait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.rust.codegen.core.smithy.protocols.AwsJsonVersion
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

class ServerProtocolLoaderTest {
    private val testModel =
        """
        ${"$"}version: "2"

        namespace test

        use aws.api#service
        use aws.protocols#awsJson1_0

        @awsJson1_0
        @service(
            sdkId: "Test",
            arnNamespace: "test"
        )
        service TestService {
            version: "2024-04-01"
        }
        """.asSmithyModel(smithyVersion = "2.0")

    private val testModelNoProtocol =
        """
        ${"$"}version: "2"

        namespace test

        use aws.api#service

        @service(
            sdkId: "Test",
            arnNamespace: "test"
        )
        service TestService {
            version: "2024-04-01"
        }
        """.asSmithyModel(smithyVersion = "2.0")

    @Test
    fun `ensures protocols are matched`() {
        val loader = ServerProtocolLoader(ServerProtocolLoader.DefaultProtocols)

        val (shape, _) = loader.protocolFor(testModel, testModel.serviceShapes.first())

        shape.name shouldBe "awsJson1_0"
    }

    @Test
    fun `ensures unmatched service protocol fails`() {
        val loader =
            ServerProtocolLoader(
                mapOf(
                    RestJson1Trait.ID to
                        ServerRestJsonFactory(
                            additionalServerHttpBoundProtocolCustomizations =
                                listOf(
                                    StreamPayloadSerializerCustomization(),
                                ),
                        ),
                    RestXmlTrait.ID to
                        ServerRestXmlFactory(
                            additionalServerHttpBoundProtocolCustomizations =
                                listOf(
                                    StreamPayloadSerializerCustomization(),
                                ),
                        ),
                    AwsJson1_1Trait.ID to
                        ServerAwsJsonFactory(
                            AwsJsonVersion.Json11,
                            additionalServerHttpBoundProtocolCustomizations = listOf(StreamPayloadSerializerCustomization()),
                        ),
                ),
            )
        val exception =
            shouldThrow<CodegenException> {
                loader.protocolFor(testModel, testModel.serviceShapes.first())
            }
        exception.message shouldContain("Unable to find a matching protocol")
    }

    @Test
    fun `ensures service without protocol fails`() {
        val loader = ServerProtocolLoader(ServerProtocolLoader.DefaultProtocols)
        val exception =
            shouldThrow<CodegenException> {
                loader.protocolFor(testModelNoProtocol, testModelNoProtocol.serviceShapes.first())
            }
        exception.message shouldContain("Service must have a protocol trait")
    }
}
