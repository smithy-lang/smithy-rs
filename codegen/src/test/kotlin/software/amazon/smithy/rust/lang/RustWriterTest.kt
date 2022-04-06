/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
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
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.docs
import software.amazon.smithy.rust.codegen.rustlang.isEmpty
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.compileAndRun
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.testutil.shouldCompile
import software.amazon.smithy.rust.codegen.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.util.lookup

class RustWriterTest {
    @Test
    fun `inner modules correctly handle dependencies`() {
        val sut = RustWriter.forModule("parent")
        val requestBuilder = RuntimeType.HttpRequestBuilder
        sut.withModule("inner") {
            rustBlock("fn build(builder: #T)", requestBuilder) {
            }
        }
        val httpDep = CargoDependency.Http.dependencies[0]
        sut.dependencies shouldContain httpDep
        sut.toString() shouldContainOnlyOnce "DO NOT EDIT"
    }

    @Test
    fun `manually created struct`() {
        val sut = RustWriter.forModule("lib")
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
        sut.rustBlock("struct Test") {
            write("member: #T,", setSymbol)
            write("otherMember: #T,", stringSymbol)
        }
        val output = sut.toString()
        output.shouldCompile()
        output shouldContain RustType.HashSet.Type
        output shouldContain "struct Test"
        output.compileAndRun(
            """
            let test = Test { member: ${RustType.HashSet.Namespace}::${RustType.HashSet.Type}::default(), otherMember: "hello".to_string() };
            assert_eq!(test.otherMember, "hello");
            assert_eq!(test.member.is_empty(), true);
            """
        )
    }

    @Test
    fun `generate docs`() {
        val sut = RustWriter.forModule("lib")
        sut.docs(
            """Top level module documentation
            |More docs
            |/* handle weird characters */
            |`a backtick`
            |[a link](asdf)
            """.trimMargin()
        )
        sut.rustBlock("pub fn foo()") { }
        sut.compileAndTest()
        sut.toString() shouldContain "Top level module"
    }

    @Test
    fun `generate doc links`() {
        val model = """
            namespace test
            structure Foo {}
        """.asSmithyModel()
        val shape = model.lookup<StructureShape>("test#Foo")
        val symbol = testSymbolProvider(model).toSymbol(shape)
        val sut = RustWriter.forModule("lib")
        sut.docs("A link! #D", symbol)
        sut.toString() shouldContain "/// A link! [`Foo`](crate::model::Foo)"
    }

    @Test
    fun `empty writable`() {
        val w = writable {}
        w.isEmpty() shouldBe true
    }
}
