/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.rustlang

import software.amazon.smithy.codegen.core.ReservedWordSymbolProvider
import software.amazon.smithy.codegen.core.ReservedWords
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.traits.EnumDefinition
import software.amazon.smithy.rust.codegen.smithy.MaybeRenamed
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.util.toPascalCase

class RustReservedWordSymbolProvider(private val base: RustSymbolProvider) : WrappingSymbolProvider(base) {
    private val internal =
        ReservedWordSymbolProvider.builder().symbolProvider(base).memberReservedWords(RustReservedWords).build()

    override fun toMemberName(shape: MemberShape): String {
        return internal.toMemberName(shape)
    }

    override fun toSymbol(shape: Shape): Symbol {
        return internal.toSymbol(shape)
    }

    override fun toEnumVariantName(definition: EnumDefinition): MaybeRenamed? {
        val baseName = base.toEnumVariantName(definition) ?: return null
        check(baseName.name.toPascalCase() == baseName.name) {
            "Enum variants must already be in pascal case"
        }
        check(baseName.renamedFrom == null) {
            "definitions should only pass through the renamer once"
        }
        return when (baseName.name) {
            // Self cannot be used as a raw identifier, so we can't use the normal escaping strategy
            // https://internals.rust-lang.org/t/raw-identifiers-dont-work-for-all-identifiers/9094/4
            "Self" -> MaybeRenamed("SelfValue", "Self")
            // Real models won't end in `_` so it's safe to stop here
            "SelfValue" -> MaybeRenamed("SelfValue_", "SelfValue")
            // Unknown is used as the name of the variant containing unexpected values
            "Unknown" -> MaybeRenamed("UnknownValue", "Unknown")
            // Real models won't end in `_` so it's safe to stop here
            "UnknownValue" -> MaybeRenamed("UnknownValue_", "UnknownValue")
            else -> baseName
        }
    }
}

object RustReservedWords : ReservedWords {
    private val RustKeywords = setOf(
        "as",
        "break",
        "const",
        "continue",
        "crate",
        "else",
        "enum",
        "extern",
        "false",
        "fn",
        "for",
        "if",
        "impl",
        "in",
        "let",
        "loop",
        "match",
        "mod",
        "move",
        "mut",
        "pub",
        "ref",
        "return",
        "self",
        "Self",
        "static",
        "struct",
        "super",
        "trait",
        "true",
        "type",
        "unsafe",
        "use",
        "where",
        "while",

        "async",
        "await",
        "dyn",

        "abstract",
        "become",
        "box",
        "do",
        "final",
        "macro",
        "override",
        "priv",
        "typeof",
        "unsized",
        "virtual",
        "yield",
        "try"
    )

    private val cantBeRaw = setOf("self", "crate", "super")

    override fun escape(word: String): String = when {
        cantBeRaw.contains(word) -> "${word}_"
        else -> "r##$word"
    }

    override fun isReserved(word: String): Boolean = RustKeywords.contains(word)
}
