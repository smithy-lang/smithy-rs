/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.ec2

import io.kotest.matchers.shouldBe
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.CsvSource
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.lookup

internal class EC2MakePrimitivesOptionalTest {
    @ParameterizedTest
    @CsvSource(
        "CLIENT",
        "CLIENT_CAREFUL",
        "CLIENT_ZERO_VALUE_V1",
        "CLIENT_ZERO_VALUE_V1_NO_INPUT",
    )
    fun `primitive shapes are boxed`(nullabilityCheckMode: NullableIndex.CheckMode) {
        val baseModel =
            """
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
        val model = EC2MakePrimitivesOptional.processModel(baseModel)
        val nullableIndex = NullableIndex(model)
        val struct = model.lookup<StructureShape>("test#Primitives")
        struct.members().forEach {
            nullableIndex.isMemberNullable(it, nullabilityCheckMode) shouldBe true
        }
    }
}
