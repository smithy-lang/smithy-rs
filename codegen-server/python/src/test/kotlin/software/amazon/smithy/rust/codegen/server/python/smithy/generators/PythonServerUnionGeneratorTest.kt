/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.util.lookup

internal class PythonServerUnionGeneratorTest {

    @Test
    fun `python union generator works`() {
        val model = """
            namespace test

            union MyUnion {
                int: PrimitiveInteger,
                union: YourUnion
            }

            union YourUnion {
                str: String,
                struct: Struct,
            }

            structure Struct {
                str: String,
            }
        """.asSmithyModel()
        val provider = testSymbolProvider(model)
        val writer = RustWriter.forModule("model")
        PythonServerStructureGenerator(model, provider, writer, model.lookup("test#Struct")).render()
        PythonServerUnionGenerator(model, provider, writer, model.lookup("test#YourUnion")).render()
        PythonServerUnionGenerator(model, provider, writer, model.lookup("test#MyUnion")).render()

        println(writer.toString())

        writer.compileAndTest()
    }
}
