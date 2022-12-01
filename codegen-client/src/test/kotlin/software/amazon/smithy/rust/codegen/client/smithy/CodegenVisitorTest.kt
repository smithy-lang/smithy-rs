/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.client.smithy.customizations.ClientCustomizations
import software.amazon.smithy.rust.codegen.client.smithy.customize.CombinedCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.NoOpEventStreamSigningDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.RequiredCustomizations
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientDecorator
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.generatePluginContext
import kotlin.io.path.createDirectory
import kotlin.io.path.writeText

class CodegenVisitorTest {
    @Test
    fun `baseline transform verify mixins removed`() {
        val model = """
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
        testDir.resolve("src").createDirectory()
        testDir.resolve("src/main.rs").writeText("fn main() {}")
        val codegenDecorator =
            CombinedCodegenDecorator.fromClasspath(
                ctx,
                ClientCustomizations(),
                RequiredCustomizations(),
                FluentClientDecorator(),
                NoOpEventStreamSigningDecorator(),
            )
        val visitor = CodegenVisitor(ctx, codegenDecorator)
        val baselineModel = visitor.baselineTransform(model)
        baselineModel.getShapesWithTrait(ShapeId.from("smithy.api#mixin")).isEmpty() shouldBe true
    }

    @Test
    fun `baseline transform verify string enum converted to EnumShape`() {
        val model = """
            namespace com.example
            use aws.protocols#restJson1
            @restJson1
            service Example {
                operations: [ BasicOperation ]
            }
            operation BasicOperation {
                input: Shape
            }
            structure Shape {
                enum: BasicEnum
            }
            @enum([
                {
                    value: "a0",
                },
                {
                    value: "a1",
                }
            ])
            string BasicEnum
        """.asSmithyModel(smithyVersion = "2.0")
        val (ctx, _) = generatePluginContext(model)
        val codegenDecorator =
            CombinedCodegenDecorator.fromClasspath(
                ctx,
                ClientCustomizations(),
            )
        val visitor = CodegenVisitor(ctx, codegenDecorator)
        val baselineModel = visitor.baselineTransform(model)
        baselineModel.getShapesWithTrait(EnumTrait.ID).isEmpty() shouldBe true
    }

    @Test
    fun `baseline transform verify bad string enum throws if not converted to EnumShape`() {
        val model = """
            namespace com.example
            use aws.protocols#restJson1
            @restJson1
            service Example {
                operations: [ BasicOperation ]
            }
            operation BasicOperation {
                input: Shape
            }
            structure Shape {
                enum: BasicEnum
            }
            @enum([
                {
                    value: "$ a/0",
                },
            ])
            string BasicEnum
        """.asSmithyModel(smithyVersion = "2.0")
        val (ctx, _) = generatePluginContext(model)
        val codegenDecorator =
            CombinedCodegenDecorator.fromClasspath(
                ctx,
                ClientCustomizations(),
            )
        val visitor = CodegenVisitor(ctx, codegenDecorator)
        val baselineModel = visitor.baselineTransform(model)
        baselineModel.getShapesWithTrait(EnumTrait.ID).isEmpty() shouldBe false
    }
}
