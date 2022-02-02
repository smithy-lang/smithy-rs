/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.rustlang

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.traits.EnumDefinition
import software.amazon.smithy.rust.codegen.smithy.MaybeRenamed
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.SymbolVisitorConfig
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.util.PANIC
import software.amazon.smithy.rust.codegen.util.orNull
import software.amazon.smithy.rust.codegen.util.toPascalCase

internal class RustReservedWordSymbolProviderTest {
    class Stub : RustSymbolProvider {
        override fun config(): SymbolVisitorConfig {
            PANIC("")
        }

        override fun toEnumVariantName(definition: EnumDefinition): MaybeRenamed? {
            return definition.name.orNull()?.let { MaybeRenamed(it.toPascalCase(), null) }
        }

        override fun toSymbol(shape: Shape): Symbol {
            return Symbol.builder().name(shape.id.name).build()
        }

        override fun isRequiredTraitHandled(member: MemberShape, useNullableIndex: Boolean): Boolean {
            return false
        }
    }

    @Test
    fun `member names are escaped`() {
        val model = """
            namespace namespace
            structure container {
                async: String
            }
        """.trimMargin().asSmithyModel()
        val provider = RustReservedWordSymbolProvider(Stub(), model)
        provider.toMemberName(
            MemberShape.builder().id("namespace#container\$async").target("namespace#Integer").build()
        ) shouldBe "r##async"

        provider.toMemberName(
            MemberShape.builder().id("namespace#container\$self").target("namespace#Integer").build()
        ) shouldBe "self_"
    }

    @Test
    fun `enum variant names are updated to avoid conflicts`() {
        expectEnumRename("Unknown", MaybeRenamed("UnknownValue", "Unknown"))
        expectEnumRename("UnknownValue", MaybeRenamed("UnknownValue_", "UnknownValue"))
        expectEnumRename("UnknownOther", MaybeRenamed("UnknownOther", null))

        expectEnumRename("Self", MaybeRenamed("SelfValue", "Self"))
        expectEnumRename("SelfValue", MaybeRenamed("SelfValue_", "SelfValue"))
        expectEnumRename("SelfOther", MaybeRenamed("SelfOther", null))
        expectEnumRename("SELF", MaybeRenamed("SelfValue", "Self"))
    }

    private fun expectEnumRename(original: String, expected: MaybeRenamed) {
        val model = "namespace foo".asSmithyModel()
        val provider = RustReservedWordSymbolProvider(Stub(), model)
        provider.toEnumVariantName(EnumDefinition.builder().name(original).value("foo").build()) shouldBe expected
    }
}
