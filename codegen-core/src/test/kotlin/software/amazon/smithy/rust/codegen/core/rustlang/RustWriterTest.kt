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
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.deprecated
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.TestModuleDocProvider
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup

class RustWriterTest {
    @Test
    fun `inner modules correctly handle dependencies`() {
        val sut = RustWriter.forModule("parent")
        val requestBuilder = RuntimeType.HttpRequestBuilder0x
        sut.withInlineModule(
            RustModule.new("inner", visibility = Visibility.PUBLIC, inline = true),
            TestModuleDocProvider,
        ) {
            rustBlock("fn build(builder: #T)", requestBuilder) {
            }
        }
        val httpDep = CargoDependency.Http0x.dependencies[0]
        sut.dependencies shouldContain httpDep
        sut.toString() shouldContainOnlyOnce "DO NOT EDIT"
    }

    @Test
    fun `manually created struct`() {
        val stringShape = StringShape.builder().id("test#Hello").build()
        val set =
            SetShape.builder()
                .id("foo.bar#Records")
                .member(stringShape.id)
                .build()
        val model =
            Model.assembler()
                .addShapes(set, stringShape)
                .assemble()
                .unwrap()

        val provider = testSymbolProvider(model)
        val setSymbol = provider.toSymbol(set)
        val stringSymbol = provider.toSymbol(stringShape)

        TestWorkspace.testProject(provider)
            .unitTest {
                rustBlock("struct Test") {
                    write("member: #T,", setSymbol)
                    write("other_member: #T,", stringSymbol)
                }

                unitTest("test_manually_created_struct") {
                    rust(
                        """
                        let test = Test { member: ${RustType.HashSet.Namespace}::${RustType.HashSet.Type}::default(), other_member: "hello".to_string() };
                        assert_eq!(test.other_member, "hello");
                        assert_eq!(test.member.is_empty(), true);

                        // If this compiles, then we know the symbol provider resolved the correct type for a set
                        let _isVec: Vec<_> = test.member;
                        """,
                    )
                }
            }.compileAndTest(runClippy = true)
    }

    @Test
    fun `generate docs`() {
        val writer = RustWriter.root()
        writer.docs(
            """Top level module documentation
            |More docs
            |/* handle weird characters */
            |`a backtick`
            |[a link](some_url)
            """,
        )
        writer.rustBlock("pub fn foo()") { }
        val output = writer.toString()
        output shouldContain "/// Top level module"
    }

    @Test
    fun `normalize HTML`() {
        val output =
            normalizeHtml(
                """
                <a>Link without href attribute</a>
                <div>Some text with [brackets]</div>
                <span>] mismatched [ is escaped too</span>
                """,
            )
        output shouldContain "<code>Link without href attribute</code>"
        output shouldContain "Some text with \\[brackets\\]"
        output shouldContain "\\] mismatched \\[ is escaped too"
    }

    @Test
    fun `generate doc links`() {
        val model =
            """
            namespace test
            structure Foo {}
            """.asSmithyModel()
        val shape = model.lookup<StructureShape>("test#Foo")
        val symbol = testSymbolProvider(model).toSymbol(shape)
        val writer = RustWriter.root()
        writer.docs("A link! #D", symbol)
        val output = writer.toString()
        output shouldContain "/// A link! [`Foo`](crate::test_model::Foo)"
    }

    @Test
    fun `empty writable`() {
        val w = writable {}
        w.isEmpty() shouldBe true
    }

    @Test
    fun `attributes with comments when using rust`() {
        val sut = RustWriter.root()
        Attribute("foo").render(sut)
        sut.rust(" // here's an attribute")
        sut.toString().shouldContain("#[foo]\n// here's an attribute")
    }

    @Test
    fun `attributes with comments when using docs`() {
        val sut = RustWriter.root()
        Attribute("foo").render(sut)
        sut.docs("here's an attribute")
        sut.toString().shouldContain("#[foo]\n/// here's an attribute")
    }

    @Test
    fun `attributes with derive helpers must come after derives`() {
        val attr = Attribute("foo", isDeriveHelper = true)
        val metadata =
            RustMetadata(
                derives = setOf(RuntimeType.Debug),
                additionalAttributes = listOf(Attribute.AllowDeprecated, attr),
                visibility = Visibility.PUBLIC,
            )
        val sut = RustWriter.root()
        metadata.render(sut)
        sut.toString().shouldContain("#[allow(deprecated)]\n#[derive(::std::fmt::Debug)]\n#[foo]")
    }

    @Test
    fun `deprecated attribute without any field`() {
        val sut = RustWriter.root()
        Attribute.Deprecated.render(sut)
        sut.toString() shouldContain "#[deprecated]"
    }

    @Test
    fun `deprecated attribute with a note`() {
        val sut = RustWriter.root()
        Attribute(deprecated(note = "custom")).render(sut)
        sut.toString() shouldContain "#[deprecated(note = \"custom\")]"
    }

    @Test
    fun `deprecated attribute with a since`() {
        val sut = RustWriter.root()
        Attribute(deprecated(since = "1.2.3")).render(sut)
        sut.toString() shouldContain "#[deprecated(since = \"1.2.3\")]"
    }

    @Test
    fun `deprecated attribute with a note and a since`() {
        val sut = RustWriter.root()
        Attribute(deprecated("1.2.3", "custom")).render(sut)
        sut.toString() shouldContain "#[deprecated(note = \"custom\", since = \"1.2.3\")]"
    }

    @Test
    fun `template writables with upper case names`() {
        val inner = writable { rust("hello") }
        val sut = RustWriter.root()
        sut.rustTemplate(
            "inner: #{Inner:W}, regular: #{http}",
            "Inner" to inner,
            "http" to RuntimeType.Http0x.resolve("foo"),
        )
        sut.toString().shouldContain("inner: hello, regular: ::http::foo")
    }

    @Test
    fun `missing template parameters are enclosed in backticks in the exception message`() {
        val sut = RustWriter.root()
        val exception =
            assertThrows<CodegenException> {
                sut.rustTemplate(
                    "#{Foo} #{Bar}",
                    "Foo Bar" to CargoDependency.Http0x.toType().resolve("foo"),
                    "Baz" to CargoDependency.Http0x.toType().resolve("foo"),
                )
            }
        exception.message shouldBe
            """
            Rust block template expected `Foo` but was not present in template.
            Hint: Template contains: [`Foo Bar`, `Baz`]
            """.trimIndent()
    }

    @Test
    fun `can handle file paths properly when determining module`() {
        val sut = RustWriter.forModule("src/module_name")
        sut.module().shouldBe("module_name")
    }
}
