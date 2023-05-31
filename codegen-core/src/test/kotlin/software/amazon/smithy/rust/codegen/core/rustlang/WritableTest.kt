/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.rustlang

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

internal class RustTypeParametersTest {
    private fun forInputExpectOutput(input: Any, expectedOutput: String) {
        val writer = RustWriter.forModule("model")
        writer.rustInlineTemplate("'")
        writer.rustInlineTemplate("#{typeParameters:W}", "typeParameters" to rustTypeParameters(input))
        writer.rustInlineTemplate("'")

        writer.toString() shouldContain expectedOutput
    }

    @Test
    fun `rustTypeParameters accepts RustType Unit`() {
        forInputExpectOutput(RustType.Unit, "()")
    }

    @Test
    fun `rustTypeParameters accepts Symbol`() {
        val symbol = RuntimeType("crate::operation::Operation").toSymbol()
        forInputExpectOutput(symbol, "'<crate::operation::Operation>'")
    }

    @Test
    fun `rustTypeParameters accepts RuntimeType`() {
        val runtimeType = RuntimeType.String
        forInputExpectOutput(runtimeType, "'<::std::string::String>'")
    }

    @Test
    fun `rustTypeParameters accepts String`() {
        forInputExpectOutput("Option<Vec<String>>", "'<Option<Vec<String>>>'")
    }

    @Test
    fun `rustTypeParameters accepts RustGenerics`() {
        forInputExpectOutput(RustGenerics(GenericTypeArg("A"), GenericTypeArg("B")), "'<A, B>'")
    }

    @Test
    fun `rustTypeParameters accepts heterogeneous inputs`() {
        val writer = RustWriter.forModule("model")
        val tps = rustTypeParameters(
            RuntimeType("crate::operation::Operation").toSymbol(),
            RustType.Unit,
            RuntimeType.String,
            "T",
            RustGenerics(GenericTypeArg("A"), GenericTypeArg("B")),
        )
        writer.rustInlineTemplate("'")
        writer.rustInlineTemplate("#{tps:W}", "tps" to tps)
        writer.rustInlineTemplate("'")

        writer.toString() shouldContain "'<crate::operation::Operation, (), ::std::string::String, T, A, B>'"
    }

    @Test
    fun `rustTypeParameters accepts writables`() {
        val writer = RustWriter.forModule("model")
        val tp = rustTypeParameters(RustType.Unit.writable)
        writer.rustInlineTemplate("'")
        writer.rustInlineTemplate("#{tp:W}", "tp" to tp)
        writer.rustInlineTemplate("'")

        writer.toString() shouldContain "'<()>'"
    }

    @Test
    fun `join iterable`() {
        val writer = RustWriter.forModule("model")
        val itemsA = listOf(writable("a"), writable("b"), writable("c")).join(", ")
        val itemsB = listOf(writable("d"), writable("e"), writable("f")).join(writable(", "))
        writer.rustTemplate("vec![#{ItemsA:W}, #{ItemsB:W}]", "ItemsA" to itemsA, "ItemsB" to itemsB)
        writer.toString() shouldContain "vec![a, b, c, d, e, f]"
    }

    @Test
    fun `join array`() {
        val writer = RustWriter.forModule("model")
        arrayOf(writable("A"), writable("B"), writable("C")).join("-")(writer)
        arrayOf(writable("D"), writable("E"), writable("F")).join(writable("+"))(writer)
        writer.toString() shouldContain "A-B-CD+E+F"
    }

    @Test
    fun `join sequence`() {
        val writer = RustWriter.forModule("model")
        sequence {
            yield(writable("A"))
            yield(writable("B"))
            yield(writable("C"))
        }.join("-")(writer)
        sequence {
            yield(writable("D"))
            yield(writable("E"))
            yield(writable("F"))
        }.join(writable("+"))(writer)
        writer.toString() shouldContain "A-B-CD+E+F"
    }
}
