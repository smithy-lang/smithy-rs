/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import io.kotest.matchers.shouldBe
import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.generatePluginContext
import software.amazon.smithy.rust.codegen.server.smithy.customizations.ServerRequiredCustomizations
import software.amazon.smithy.rust.codegen.server.smithy.customizations.SmithyValidationExceptionDecorator
import software.amazon.smithy.rust.codegen.server.smithy.customize.CombinedServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerProtocolMap
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocolGenerator
import kotlin.io.path.writeText

class ServerCodegenVisitorTest {
    @Test
    fun `legacy generation diagnoses a non-server primary protocol`() =
        checkNonServerProtocol(schemaSerde = false, secondaryProtocol = false)

    @Test
    fun `schema generation diagnoses a non-server primary protocol`() =
        checkNonServerProtocol(schemaSerde = true, secondaryProtocol = false)

    @Test
    fun `schema marker resolution diagnoses a non-server secondary protocol`() =
        checkNonServerProtocol(schemaSerde = true, secondaryProtocol = true)

    private fun checkNonServerProtocol(
        schemaSerde: Boolean,
        secondaryProtocol: Boolean,
    ) {
        val model =
            """
            namespace test
            use aws.protocols#restJson1
            use aws.protocols#awsJson1_1
            @restJson1
            ${if (secondaryProtocol) "@awsJson1_1" else ""}
            service Example { version: "1", operations: [] }
            """.asSmithyModel()
        val invalidProtocol = ShapeId.from("aws.protocols#${if (secondaryProtocol) "awsJson1_1" else "restJson1"}")
        val decorator =
            object : ServerCodegenDecorator {
                override val name = "Non-server protocol factory"
                override val order: Byte = 0

                override fun protocols(
                    serviceId: ShapeId,
                    currentProtocols: ServerProtocolMap,
                ): ServerProtocolMap =
                    currentProtocols + (invalidProtocol to NonServerProtocolFactory(currentProtocols.getValue(invalidProtocol)))
            }
        val (context, _) =
            generatePluginContext(
                model,
                additionalSettings =
                    Node.objectNode().withMember(
                        "codegen",
                        Node.objectNode().withMember("schemaSerde", schemaSerde).withMember("http-1x", true),
                    ),
            )
        val visitor =
            ServerCodegenVisitor(
                context,
                CombinedServerCodegenDecorator.fromClasspath(
                    context,
                    ServerRequiredCustomizations(),
                    SmithyValidationExceptionDecorator(),
                    decorator,
                ),
            )
        val exception =
            assertThrows<CodegenException> {
                visitor.serviceShape(model.expectShape(ShapeId.from("test#Example"), ServiceShape::class.java))
            }
        exception.message shouldContain invalidProtocol.toString()
        exception.message shouldContain NonServerProtocolFactory::class.java.name
        exception.message shouldContain NonServerProtocol::class.java.name
        exception.message shouldContain "Server protocol factories must return a ServerProtocol implementation"
    }

    private class NonServerProtocol(delegate: Protocol) : Protocol by delegate

    private class NonServerProtocolFactory(
        private val delegate: ProtocolGeneratorFactory<ServerProtocolGenerator, ServerCodegenContext>,
    ) : ProtocolGeneratorFactory<ServerProtocolGenerator, ServerCodegenContext> by delegate {
        override fun protocol(codegenContext: ServerCodegenContext): Protocol =
            NonServerProtocol(delegate.protocol(codegenContext))
    }

    @Test
    fun `baseline transform verify mixins removed`() {
        val model =
            """
            namespace com.example

            use aws.protocols#awsJson1_0

            @awsJson1_0
            @aws.api#service(sdkId: "Test", endpointPrefix: "differentPrefix")
            service Example {
                operations: [ BasicOperation ]
            }

            operation BasicOperation {
                input: Shape
            }

            @mixin
            structure SimpleMixin {
                name: String
            }

            structure Shape with [
                SimpleMixin
            ] {
                greeting: String
            }
            """.asSmithyModel(smithyVersion = "2.0")
        val (ctx, testDir) = generatePluginContext(model)
        testDir.resolve("src/main.rs").writeText("fn main() {}")
        val codegenDecorator =
            CombinedServerCodegenDecorator.fromClasspath(
                ctx,
                ServerRequiredCustomizations(),
                SmithyValidationExceptionDecorator(),
            )
        val visitor = ServerCodegenVisitor(ctx, codegenDecorator)
        val baselineModel = visitor.baselineTransformInternalTest(model)
        baselineModel.getShapesWithTrait(ShapeId.from("smithy.api#mixin")).isEmpty() shouldBe true
    }
}
