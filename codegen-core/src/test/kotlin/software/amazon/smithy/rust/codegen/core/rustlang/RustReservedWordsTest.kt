/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.rustlang

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.smithy.MaybeRenamed
import software.amazon.smithy.rust.codegen.core.smithy.SymbolVisitor
import software.amazon.smithy.rust.codegen.core.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.renamedFrom
import software.amazon.smithy.rust.codegen.core.testutil.TestSymbolVisitorConfig
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

internal class RustReservedWordSymbolProviderTest {
    private class TestSymbolProvider(override val model: Model) :
        WrappingSymbolProvider(SymbolVisitor(model, TestSymbolVisitorConfig, null))

    @Test
    fun `member names are escaped`() {
        val model = """
            namespace namespace
            structure container {
                async: String
            }
        """.trimMargin().asSmithyModel()
        val provider = RustReservedWordSymbolProvider(TestSymbolProvider(model))
        provider.toMemberName(
            MemberShape.builder().id("namespace#container\$async").target("namespace#Integer").build(),
        ) shouldBe "r##async"

        provider.toMemberName(
            MemberShape.builder().id("namespace#container\$self").target("namespace#Integer").build(),
        ) shouldBe "self_"
    }

    @Test
    fun `enum variant names are updated to avoid conflicts`() {
        val model = """
            namespace foo
            @enum([{ name: "dontcare", value: "dontcare" }]) string Container
        """.asSmithyModel()
        val provider = RustReservedWordSymbolProvider(TestSymbolProvider(model))

        fun expectEnumRename(original: String, expected: MaybeRenamed) {
            val symbol = provider.toSymbol(
                MemberShape.builder()
                    .id(ShapeId.fromParts("foo", "Container").withMember(original))
                    .target("smithy.api#String")
                    .build(),
            )
            symbol.name shouldBe expected.name
            symbol.renamedFrom() shouldBe expected.renamedFrom
        }

        expectEnumRename("Unknown", MaybeRenamed("UnknownValue", "Unknown"))
        expectEnumRename("UnknownValue", MaybeRenamed("UnknownValue_", "UnknownValue"))
        expectEnumRename("UnknownOther", MaybeRenamed("UnknownOther", null))

        expectEnumRename("Self", MaybeRenamed("SelfValue", "Self"))
        expectEnumRename("SelfValue", MaybeRenamed("SelfValue_", "SelfValue"))
        expectEnumRename("SelfOther", MaybeRenamed("SelfOther", null))
        expectEnumRename("SELF", MaybeRenamed("SelfValue", "Self"))
    }
}
