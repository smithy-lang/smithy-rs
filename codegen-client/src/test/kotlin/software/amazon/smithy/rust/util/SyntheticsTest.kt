/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.util

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.util.cloneOperation
import software.amazon.smithy.rust.codegen.util.orNull
import software.amazon.smithy.rust.codegen.util.rename

class SyntheticsTest {
    @Test
    fun `it should clone operations`() {
        val model = """
            namespace test

            service TestService {
                version: "2019-12-16",
                operations: ["SomeOperation"],
            }

            structure TestInput {
                one: String,
                two: String,
            }
            structure TestOutput {
                three: String,
                four: String,
            }

            operation SomeOperation { input: TestInput, output: TestOutput }
        """.asSmithyModel()

        val transformed = model.toBuilder().cloneOperation(model, ShapeId.from("test#SomeOperation")) { shapeId ->
            ShapeId.fromParts(shapeId.namespace + ".cloned", shapeId.name + "Foo")
        }.build()

        val newOp = transformed.expectShape(ShapeId.from("test.cloned#SomeOperationFoo"), OperationShape::class.java)
        newOp.input.orNull() shouldBe ShapeId.from("test.cloned#TestInputFoo")
        newOp.output.orNull() shouldBe ShapeId.from("test.cloned#TestOutputFoo")

        val newIn = transformed.expectShape(ShapeId.from("test.cloned#TestInputFoo"), StructureShape::class.java)
        for (member in newIn.members()) {
            member.id shouldBe ShapeId.fromParts("test.cloned", "TestInputFoo", member.memberName)
        }

        val newOut = transformed.expectShape(ShapeId.from("test.cloned#TestOutputFoo"), StructureShape::class.java)
        for (member in newOut.members()) {
            member.id shouldBe ShapeId.fromParts("test.cloned", "TestOutputFoo", member.memberName)
        }
    }

    @Test
    fun `it should rename structs`() {
        val model = """
            namespace test

            structure SomeInput {
                one: String,
                two: String,
            }
        """.asSmithyModel()

        val original = model.expectShape(ShapeId.from("test#SomeInput"), StructureShape::class.java)
        val new = original.toBuilder().rename(ShapeId.from("new#SomeOtherInput")).build()
        new.id shouldBe ShapeId.from("new#SomeOtherInput")
        for (member in new.members()) {
            member.id shouldBe ShapeId.fromParts("new", "SomeOtherInput", member.memberName)
        }
    }
}
