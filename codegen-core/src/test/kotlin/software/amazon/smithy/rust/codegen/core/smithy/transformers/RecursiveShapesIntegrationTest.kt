/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.transformers

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.util.CommandError
import software.amazon.smithy.rust.codegen.core.util.lookup

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
            val symbolProvider = testSymbolProvider(model)
            val project = TestWorkspace.testProject(symbolProvider)
            val structures = listOf("Expr", "SecondTree").map { input.lookup<StructureShape>("com.example#$it") }
            structures.forEach { struct ->
                project.moduleFor(struct) {
                    StructureGenerator(input, symbolProvider, this, struct, emptyList()).render()
                }
            }
            input.lookup<UnionShape>("com.example#Atom").also { atom ->
                project.moduleFor(atom) {
                    UnionGenerator(input, symbolProvider, this, atom).render()
                }
            }
            project
        }
        val unmodifiedProject = check(model)
        val output = assertThrows<CommandError> {
            unmodifiedProject.compileAndTest(expectFailure = true)
        }
        // THIS IS A LOAD-BEARING shouldContain! If the compiler error changes then this will break!
        output.message shouldContain "have infinite size"

        val fixedProject = check(RecursiveShapeBoxer().transform(model))
        fixedProject.compileAndTest()
    }
}
