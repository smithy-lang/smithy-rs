/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext

internal class PythonTypeInformationGenerationTest {
    @Test
    fun `generates python type information`() {
        val model =
            """
            namespace test

            structure Foo {
                @required
                bar: String,
                baz: Integer
            }
            """.asSmithyModel()
        val foo = model.lookup<StructureShape>("test#Foo")

        val codegenContext = serverTestCodegenContext(model)
        val writer = RustWriter.forModule("model")
        PythonServerStructureGenerator(model, codegenContext, writer, foo).render()

        val result = writer.toString()

        // Constructor signature
        result.shouldContain("/// :param bar str:")
        result.shouldContain("/// :param baz typing.Optional\\[int\\]:")

        // Field types
        result.shouldContain("/// :type str:")
        result.shouldContain("/// :type typing.Optional\\[int\\]:")
    }
}
