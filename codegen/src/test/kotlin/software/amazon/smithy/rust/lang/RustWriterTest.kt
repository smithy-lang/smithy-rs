/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.lang

import io.kotest.matchers.collections.shouldContain
import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.lang.CargoDependency
import software.amazon.smithy.rust.codegen.lang.RustType
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.lang.docs
import software.amazon.smithy.rust.codegen.lang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.util.lookup
import software.amazon.smithy.rust.testutil.asSmithyModel
import software.amazon.smithy.rust.testutil.compileAndRun
import software.amazon.smithy.rust.testutil.compileAndTest
import software.amazon.smithy.rust.testutil.shouldCompile
import software.amazon.smithy.rust.testutil.shouldMatchResource
import software.amazon.smithy.rust.testutil.shouldParseAsRust
import software.amazon.smithy.rust.testutil.testSymbolProvider

class RustWriterTest {
    @Test
    fun `empty file`() {
        val sut = RustWriter.forModule("empty")
        sut.toString().shouldParseAsRust()
        sut.toString().shouldCompile()
        sut.toString().shouldMatchResource(javaClass, "empty.rs")
    }

    @Test
    fun `inner modules correctly handle dependencies`() {
        val sut = RustWriter.forModule("lib")
        val requestBuilder = RuntimeType.HttpRequestBuilder
        sut.withModule("inner") {
            rustBlock("fn build(builer: \$T)", requestBuilder) {
            }
        }
        val httpDep = CargoDependency.Http.dependencies[0]
        sut.dependencies shouldContain httpDep
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
            write("member: \$T,", setSymbol)
            write("otherMember: \$T,", stringSymbol)
        }
        val output = sut.toString()
        output.shouldCompile()
        output shouldContain RustType.SetType
        output shouldContain "struct Test"
        output.compileAndRun(
            """
        let test = Test { member: ${RustType.SetType}::default(), otherMember: "hello".to_string() };
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
        sut.rustBlock("fn main()") { }
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
        sut.docs("A link! \$D", symbol)
        sut.toString() shouldContain "/// A link! [`Foo`](crate::model::Foo)"
    }
}
