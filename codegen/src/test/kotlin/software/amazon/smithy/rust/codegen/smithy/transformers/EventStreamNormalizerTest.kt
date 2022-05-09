/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.transformers

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticEventStreamUnionTrait
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.util.expectTrait
import software.amazon.smithy.rust.codegen.util.hasTrait

class EventStreamNormalizerTest {
    @Test
    fun `it should leave normal unions alone`() {
        val transformed = EventStreamNormalizer.transform(
            """
            namespace test
            union SomeNormalUnion {
                Foo: String,
                Bar: Long,
            }
            """.asSmithyModel()
        )

        val shape = transformed.expectShape(ShapeId.from("test#SomeNormalUnion"), UnionShape::class.java)
        shape.hasTrait<SyntheticEventStreamUnionTrait>() shouldBe false
        shape.memberNames shouldBe listOf("Foo", "Bar")
    }

    @Test
    fun `it should transform event stream unions`() {
        val transformed = EventStreamNormalizer.transform(
            """
            namespace test

            structure SomeMember {
            }

            @error("client")
            structure SomeError {
            }

            @streaming
            union SomeEventStream {
                SomeMember: SomeMember,
                SomeError: SomeError,
            }
            """.asSmithyModel()
        )

        val shape = transformed.expectShape(ShapeId.from("test#SomeEventStream"), UnionShape::class.java)
        shape.hasTrait<SyntheticEventStreamUnionTrait>() shouldBe true
        shape.memberNames shouldBe listOf("SomeMember")

        val trait = shape.expectTrait<SyntheticEventStreamUnionTrait>()
        trait.errorMembers.map { it.memberName } shouldBe listOf("SomeError")
    }
}
