/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.ec2

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.BoxTrait
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.lookup

internal class BoxPrimitiveShapesTest {
    @Test
    fun `primitive shapes are boxed`() {
        val baseModel = """
            namespace test
            structure Primitives {
                int: PrimitiveInteger,
                bool: PrimitiveBoolean,
                long: PrimitiveLong,
                double: PrimitiveDouble,
                boxedAlready: BoxedField,
                other: Other
            }

            @box
            integer BoxedField

            structure Other {}

        """.asSmithyModel()
        val model = BoxPrimitiveShapes.processModel(baseModel)

        val struct = model.lookup<StructureShape>("test#Primitives")
        struct.members().forEach {
            val target = model.expectShape(it.target)
            when (target) {
                is StructureShape -> target.hasTrait<BoxTrait>() shouldBe false
                else -> target.hasTrait<BoxTrait>() shouldBe true
            }
        }
    }
}
