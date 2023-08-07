/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.rustlang

import io.kotest.matchers.shouldBe
import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.all
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.any
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.cfg
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.deny
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.derive
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.doc
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.not
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.dq

internal class RustTypesTest {
    private fun forInputExpectOutput(t: Writable, expectedOutput: String) {
        val writer = RustWriter.forModule("rust_types")
        writer.rustInlineTemplate("'")
        t.invoke(writer)
        writer.rustInlineTemplate("'")

        writer.toString() shouldContain expectedOutput
    }

    @Test
    fun `RustType_Unit_writable produces a template-compatible RuntimeType`() {
        forInputExpectOutput(RustType.Unit.writable, "'()'")
    }

    @Test
    fun `RustType_Bool_writable produces a template-compatible RuntimeType`() {
        forInputExpectOutput(RustType.Bool.writable, "'bool'")
    }

    @Test
    fun `RustType_Float_writable produces a template-compatible RuntimeType`() {
        forInputExpectOutput(RustType.Float(32).writable, "'f32'")
        forInputExpectOutput(RustType.Float(64).writable, "'f64'")
    }

    @Test
    fun `RustType_Integer_writable produces a template-compatible RuntimeType`() {
        forInputExpectOutput(RustType.Integer(8).writable, "'i8'")
        forInputExpectOutput(RustType.Integer(16).writable, "'i16'")
        forInputExpectOutput(RustType.Integer(32).writable, "'i32'")
        forInputExpectOutput(RustType.Integer(64).writable, "'i64'")
    }

    @Test
    fun `RustType_String_writable produces a template-compatible RuntimeType`() {
        forInputExpectOutput(RustType.String.writable, "'::std::string::String'")
    }

    @Test
    fun `RustType_Vec_writable produces a template-compatible RuntimeType`() {
        forInputExpectOutput(
            RustType.Vec(RustType.String).writable,
            "'::std::vec::Vec<::std::string::String>'",
        )
    }

    @Test
    fun `RustType_Slice_writable produces a template-compatible RuntimeType`() {
        forInputExpectOutput(
            RustType.Slice(RustType.String).writable,
            "'[::std::string::String]'",
        )
    }

    @Test
    fun `RustType_HashMap_writable produces a template-compatible RuntimeType`() {
        forInputExpectOutput(
            RustType.HashMap(RustType.String, RustType.String).writable,
            "'::std::collections::HashMap<::std::string::String, ::std::string::String>'",
        )
    }

    @Test
    fun `RustType_HashSet_writable produces a template-compatible RuntimeType`() {
        forInputExpectOutput(
            RustType.HashSet(RustType.String).writable,
            // Rust doesn't guarantee that `HashSet`s are insertion ordered, so we use a `Vec` instead.
            // This is called out in a comment in the RustType.HashSet declaration
            "'::std::vec::Vec<::std::string::String>'",
        )
    }

    @Test
    fun `RustType_Reference_writable produces a template-compatible RuntimeType`() {
        forInputExpectOutput(
            RustType.Reference("&", RustType.String).writable,
            "'&::std::string::String'",
        )
        forInputExpectOutput(
            RustType.Reference("&mut", RustType.String).writable,
            "'&mut ::std::string::String'",
        )
        forInputExpectOutput(
            RustType.Reference("&'static", RustType.String).writable,
            "&'static ::std::string::String'",
        )
    }

    @Test
    fun `RustType_Option_writable produces a template-compatible RuntimeType`() {
        forInputExpectOutput(
            RustType.Option(RustType.String).writable,
            "'::std::option::Option<::std::string::String>'",
        )
    }

    @Test
    fun `RustType_Box_writable produces a template-compatible RuntimeType`() {
        forInputExpectOutput(
            RustType.Box(RustType.String).writable,
            "'::std::boxed::Box<::std::string::String>'",
        )
    }

    @Test
    fun `RustType_Opaque_writable produces a template-compatible RuntimeType`() {
        forInputExpectOutput(
            RustType.Opaque("SoCool", "zelda_is").writable,
            "'zelda_is::SoCool'",
        )
        forInputExpectOutput(
            RustType.Opaque("SoCool").writable,
            "'SoCool'",
        )
    }

    @Test
    fun `RustType_Dyn_writable produces a template-compatible RuntimeType`() {
        forInputExpectOutput(
            RustType.Dyn(RustType.Opaque("Foo", "foo")).writable,
            "'dyn foo::Foo'",
        )
    }

    @Test
    fun `types render properly`() {
        val type = RustType.Box(RustType.Option(RustType.Reference("a", RustType.Vec(RustType.String))))
        type.render(false) shouldBe "Box<Option<&'a Vec<String>>>"
        type.render(true) shouldBe "::std::boxed::Box<::std::option::Option<&'a ::std::vec::Vec<::std::string::String>>>"
    }

    @Test
    fun `attribute macros from strings render properly`() {
        val attributeMacro = Attribute(
            Attribute.cfg(
                Attribute.all(
                    Attribute.pair("feature" to "unstable".dq()),
                    Attribute.any(
                        Attribute.pair("feature" to "serialize".dq()),
                        Attribute.pair("feature" to "deserialize".dq()),
                    ),
                ),
            ),
        )
        forInputExpectOutput(
            writable {
                attributeMacro.render(this)
            },
            "#[cfg(all(feature = \"unstable\", any(feature = \"serialize\", feature = \"deserialize\")))]\n",
        )
    }

    @Test
    fun `attribute macros render writers properly`() {
        val attributeMacro = Attribute(
            cfg(
                all(
                    // Normally we'd use the `pair` fn to define these but this is a test
                    writable { rustInline("""feature = "unstable"""") },
                    writable { rustInline("""feature = "serialize"""") },
                    writable { rustInline("""feature = "deserialize"""") },
                ),
            ),
        )
        forInputExpectOutput(
            writable {
                attributeMacro.render(this)
            },
            "#[cfg(all(feature = \"unstable\", feature = \"serialize\", feature = \"deserialize\"))]\n",
        )
    }

    @Test
    fun `attribute macros render nothing when empty`() {
        // All of these attributes require arguments. If none are supplied, then they shouldn't render at all
        val attributeMacro = Attribute(cfg(all(any(doc(not(deny()))))))
        forInputExpectOutput(writable { attributeMacro.render(this) }, "")
    }

    @Test
    fun `derive attribute macros render properly`() {
        val attributeMacro = Attribute(
            derive(
                RuntimeType.Clone,
                RuntimeType.Debug,
                RuntimeType.StdError,
            ),
        )
        forInputExpectOutput(
            writable {
                attributeMacro.render(this)
            },
            "#[derive(::std::clone::Clone, ::std::error::Error, ::std::fmt::Debug)]\n",
        )
    }

    @Test
    fun `derive attribute macros don't render when empty`() {
        val attributeMacro = Attribute(derive())
        forInputExpectOutput(writable { attributeMacro.render(this) }, "")
    }
}
