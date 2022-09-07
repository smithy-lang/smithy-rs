/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.rustlang

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.client.smithy.generators.GenericTypeArg
import software.amazon.smithy.rust.codegen.client.smithy.generators.GenericsGenerator

internal class RustTypeParametersTest {
    private fun forInputExpectOutput(input: Any, expectedOutput: String) {
        val writer = RustWriter.forModule("model")
        writer.rustTemplate("#{typeParameters:W}", "typeParameters" to rustTypeParameters(input))

        writer.toString() shouldContain expectedOutput
    }

    @Test
    fun `rustTypeParameters accepts RustType Unit`() {
        forInputExpectOutput(RustType.Unit, "()")
    }

    @Test
    fun `rustTypeParameters accepts Symbol`() {
        val symbol = RuntimeType("Operation", namespace = "crate::operation", dependency = null).toSymbol()
        forInputExpectOutput(symbol, "<crate::operation::Operation>")
    }

    @Test
    fun `rustTypeParameters accepts RuntimeType`() {
        val runtimeType = RuntimeType("String", namespace = "std::string", dependency = null)
        forInputExpectOutput(runtimeType, "<std::string::String>")
    }

    @Test
    fun `rustTypeParameters accepts String`() {
        forInputExpectOutput("Option<Vec<String>>", "<Option<Vec<String>>>")
    }

    @Test
    fun `rustTypeParameters accepts GenericsGenerator`() {
        forInputExpectOutput(GenericsGenerator(GenericTypeArg("A"), GenericTypeArg("B")), "<A, B>")
    }

    @Test
    fun `rustTypeParameters accepts heterogeneous inputs`() {
        val writer = RustWriter.forModule("model")
        val tps = rustTypeParameters(
            RuntimeType("Operation", namespace = "crate::operation", dependency = null).toSymbol(),
            RustType.Unit,
            RuntimeType("String", namespace = "std::string", dependency = null),
            "T",
            GenericsGenerator(GenericTypeArg("A"), GenericTypeArg("B")),
        )
        writer.rustTemplate("#{typeParameters:W}", "typeParameters" to tps)

        writer.toString() shouldContain "<crate::operation::Operation, (), std::string::String, T, A, B>"
    }
}
