/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.rustlang

import io.kotest.matchers.collections.shouldContain
import io.kotest.matchers.shouldBe
import io.kotest.matchers.string.shouldContain
import io.kotest.matchers.string.shouldContainOnlyOnce
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.deprecated
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndRun
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.shouldCompile
import software.amazon.smithy.rust.codegen.core.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.util.lookup

class RustWriterTest {
    @Test
    fun `inner modules correctly handle dependencies`() {
        val sut = RustWriter.forModule("parent")
        val requestBuilder = RuntimeType.HttpRequestBuilder
        sut.withInlineModule(RustModule.new("inner", visibility = Visibility.PUBLIC, inline = true)) {
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

        val provider = testSymbolProvider(model)
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
            """,
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
            """.trimMargin(),
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

    @Test
    fun `attributes with comments when using rust`() {
        val sut = RustWriter.forModule("lib")
        Attribute("foo").render(sut)
        sut.rust(" // here's an attribute")
        sut.toString().shouldContain("#[foo]\n// here's an attribute")
    }

    @Test
    fun `attributes with comments when using docs`() {
        val sut = RustWriter.forModule("lib")
        Attribute("foo").render(sut)
        sut.docs("here's an attribute")
        sut.toString().shouldContain("#[foo]\n/// here's an attribute")
    }

    @Test
    fun `deprecated attribute without any field`() {
        val sut = RustWriter.forModule("lib")
        Attribute.Deprecated.render(sut)
        sut.toString() shouldContain "#[deprecated]"
    }

    @Test
    fun `deprecated attribute with a note`() {
        val sut = RustWriter.forModule("lib")
        Attribute(deprecated(note = "custom")).render(sut)
        sut.toString() shouldContain "#[deprecated(note = \"custom\")]"
    }

    @Test
    fun `deprecated attribute with a since`() {
        val sut = RustWriter.forModule("lib")
        Attribute(deprecated(since = "1.2.3")).render(sut)
        sut.toString() shouldContain "#[deprecated(since = \"1.2.3\")]"
    }

    @Test
    fun `deprecated attribute with a note and a since`() {
        val sut = RustWriter.forModule("lib")
        Attribute(deprecated("1.2.3", "custom")).render(sut)
        sut.toString() shouldContain "#[deprecated(note = \"custom\", since = \"1.2.3\")]"
    }

    @Test
    fun `template writables with upper case names`() {
        val inner = writable { rust("hello") }
        val sut = RustWriter.forModule("lib")
        sut.rustTemplate(
            "inner: #{Inner:W}, regular: #{http}",
            "Inner" to inner,
            "http" to RuntimeType.Http.resolve("foo"),
        )
        sut.toString().shouldContain("inner: hello, regular: http::foo")
    }

    @Test
    fun `can handle file paths properly when determining module`() {
        val sut = RustWriter.forModule("src/module_name")
        sut.module().shouldBe("module_name")
    }
}
