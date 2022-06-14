/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.lang

import io.kotest.matchers.collections.shouldContain
import io.kotest.matchers.shouldBe
import io.kotest.matchers.string.shouldContain
import io.kotest.matchers.string.shouldContainOnlyOnce
import org.junit.jupiter.api.Test
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.docs
import software.amazon.smithy.rust.codegen.rustlang.isEmpty
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.testutil.unitTest
import software.amazon.smithy.rust.codegen.util.lookup

class RustWriterTest {
    @Test
    fun `inner modules correctly handle dependencies`() {
        val project = TestWorkspace.testProject()
        project.withModule(RustModule.public("parent")) {
            val requestBuilder = RuntimeType.HttpRequestBuilder

            it.withModule("inner") {
                rustBlock("fn build(builder: #T)", requestBuilder) {}
            }
            val httpDep = CargoDependency.Http.dependencies[0]
            it.dependencies shouldContain httpDep
            it.toString() shouldContainOnlyOnce "DO NOT EDIT"
        }
        project.compileAndTest()
    }

    @Test
    fun `manually created struct`() {
        val project = TestWorkspace.testProject()
        project.lib {
            val stringShape = StringShape.builder().id("test#Hello").build()
            val set = SetShape.builder()
                .id("foo.bar#Records")
                .member(stringShape.id)
                .build()
            val model = Model.assembler()
                .addShapes(set, stringShape)
                .assemble()
                .unwrap()

            val provider: SymbolProvider = testSymbolProvider(model)
            val setSymbol = provider.toSymbol(set)
            val stringSymbol = provider.toSymbol(stringShape)
            it.rustBlock("struct Test") {
                write("member: #T,", setSymbol)
                write("otherMember: #T,", stringSymbol)
            }
            it.unitTest("manually_created_struct") {
                rustTemplate(
                    """
                    let test = Test { member: #{Set}::default(), otherMember: "hello".to_string() };
                    assert_eq!(test.otherMember, "hello");
                    assert_eq!(test.member.is_empty(), true);
                    """,
                    "Set" to RuntimeType.Set(TestRuntimeConfig)
                )
            }
            val output = it.toString()
            output shouldContain RustType.HashSet.Type
            output shouldContain "struct Test"
        }
        project.compileAndTest()
    }

    @Test
    fun `generate docs`() {
        val project = TestWorkspace.testProject()
        project.lib {
            it.docs(
                """Top level module documentation
                |More docs
                |/* handle weird characters */
                |`a backtick`
                |[a link](asdf)
                """.trimMargin()
            )
            it.rustBlock("pub fn foo()") { }
            it.toString() shouldContain "Top level module"
        }
        project.compileAndTest()
    }

    @Test
    fun `generate doc links`() {
        val model = """
            namespace test
            structure Foo {}
        """.asSmithyModel()
        val symbolProvider = testSymbolProvider(model)
        val project = TestWorkspace.testProject(symbolProvider)

        project.lib {
            val shape = model.lookup<StructureShape>("test#Foo")
            val symbol = symbolProvider.toSymbol(shape)
            it.docs("A link! #D", symbol)
            it.toString() shouldContain "/// A link! [`Foo`](crate::model::Foo)"
            it.rustBlock("pub fn foo()") { }
        }
        project.compileAndTest()
    }

    @Test
    fun `empty writable`() {
        val w = writable {}
        w.isEmpty() shouldBe true
    }
}
