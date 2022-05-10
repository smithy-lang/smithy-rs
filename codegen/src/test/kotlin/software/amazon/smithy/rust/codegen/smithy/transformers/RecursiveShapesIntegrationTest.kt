/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.transformers

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.util.CommandFailed
import software.amazon.smithy.rust.codegen.util.lookup

class RecursiveShapesIntegrationTest {
    @Test
    fun `recursive shapes are properly boxed`() {
        val model = """
            namespace com.example
            structure Expr {
                 left: Atom,
                 right: Atom
            }

            union Atom {
                 add: Expr,
                 sub: Expr,
                 literal: Integer,
                 more: SecondTree
            }

            structure SecondTree {
                 member: Expr,
                 otherMember: Atom,
                 third: SecondTree
            }
        """.asSmithyModel()
        val check = { input: Model ->
            val structures = listOf("Expr", "SecondTree").map { input.lookup<StructureShape>("com.example#$it") }
            val writer = RustWriter.forModule("model")
            val symbolProvider = testSymbolProvider(input)
            structures.forEach {
                StructureGenerator(input, symbolProvider, writer, it).render()
            }
            UnionGenerator(input, symbolProvider, writer, input.lookup("com.example#Atom")).render()
            writer
        }
        val unmodifiedWriter = check(model)
        val output = assertThrows<CommandFailed> {
            unmodifiedWriter.compileAndTest(expectFailure = true)
        }
        output.message shouldContain "has infinite size"

        val fixedWriter = check(RecursiveShapeBoxer.transform(model))
        fixedWriter.compileAndTest()
    }
}
