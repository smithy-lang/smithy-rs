/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.rustlang

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

class RustGenericsTest {
    @Test
    fun `declaration is correct for no args`() {
        val gg = RustGenerics()
        val writer = RustWriter.forModule("model")
        writer.rustTemplate("A#{decl:W}B", "decl" to gg.declaration())

        writer.toString() shouldContain "AB"
    }

    @Test
    fun `declaration is correct for 1 arg`() {
        val gg = RustGenerics(GenericTypeArg("T"))
        val writer = RustWriter.forModule("model")
        writer.rustTemplate("#{decl:W}", "decl" to gg.declaration())

        writer.toString() shouldContain "<T>"
    }

    @Test
    fun `declaration is correct for several args`() {
        val gg = RustGenerics(GenericTypeArg("T"), GenericTypeArg("U"), GenericTypeArg("V"))
        val writer = RustWriter.forModule("model")
        writer.rustTemplate("#{decl:W}", "decl" to gg.declaration())

        writer.toString() shouldContain "<T, U, V>"
    }

    @Test
    fun `bounds is correct for no args`() {
        val gg = RustGenerics()
        val writer = RustWriter.forModule("model")
        writer.rustTemplate("A#{bounds:W}B", "bounds" to gg.bounds())

        writer.toString() shouldContain "AB"
    }

    @Test
    fun `bounds is correct for 1 arg`() {
        val gg = RustGenerics(GenericTypeArg("T", testRT("Test")))
        val writer = RustWriter.forModule("model")
        writer.rustTemplate("#{bounds:W}", "bounds" to gg.bounds())

        writer.toString() shouldContain "T: test::Test,"
    }

    @Test
    fun `bounds is correct for several args`() {
        val gg = RustGenerics(
            GenericTypeArg("A", testRT("Apple")),
            GenericTypeArg("PL", testRT("Plum")),
            GenericTypeArg("PE", testRT("Pear")),
        )
        val writer = RustWriter.forModule("model")
        writer.rustTemplate("#{bounds:W}", "bounds" to gg.bounds())

        writer.toString() shouldContain """
            A: test::Apple,
            PL: test::Plum,
            PE: test::Pear,
        """.trimIndent()
    }

    @Test
    fun `bounds skips arg with no bounds`() {
        val gg = RustGenerics(
            GenericTypeArg("A", testRT("Apple")),
            GenericTypeArg("PL"),
            GenericTypeArg("PE", testRT("Pear")),
        )
        val writer = RustWriter.forModule("model")
        writer.rustTemplate("#{bounds:W}", "bounds" to gg.bounds())

        writer.toString() shouldContain """
            A: test::Apple,
            PE: test::Pear,
        """.trimIndent()
    }

    @Test
    fun `bounds generates nothing if all args are skipped`() {
        val gg = RustGenerics(
            GenericTypeArg("A"),
            GenericTypeArg("PL"),
            GenericTypeArg("PE"),
        )
        val writer = RustWriter.forModule("model")
        writer.rustTemplate("A#{bounds:W}B", "bounds" to gg.bounds())

        writer.toString() shouldContain "AB"
    }

    @Test
    fun `Adding GenericGenerators works`() {
        val ggA = RustGenerics(
            GenericTypeArg("A", testRT("Apple")),
        )
        val ggB = RustGenerics(
            GenericTypeArg("B", testRT("Banana")),
        )
        RustWriter.forModule("model").let {
            it.rustTemplate("#{bounds:W}", "bounds" to (ggA + ggB).bounds())

            it.toString() shouldContain """
                A: test::Apple,
                B: test::Banana,
            """.trimIndent()
        }

        RustWriter.forModule("model").let {
            it.rustTemplate("#{decl:W}", "decl" to (ggA + ggB).declaration())

            it.toString() shouldContain "<A, B>"
        }
    }

    private fun testRT(name: String): RuntimeType = RuntimeType("test::$name")
}
