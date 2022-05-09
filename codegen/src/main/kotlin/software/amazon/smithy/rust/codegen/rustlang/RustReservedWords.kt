/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.rustlang

import software.amazon.smithy.codegen.core.ReservedWordSymbolProvider
import software.amazon.smithy.codegen.core.ReservedWords
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumDefinition
import software.amazon.smithy.rust.codegen.smithy.MaybeRenamed
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.smithy.letIf
import software.amazon.smithy.rust.codegen.smithy.renamedFrom
import software.amazon.smithy.rust.codegen.util.orNull
import software.amazon.smithy.rust.codegen.util.toPascalCase

class RustReservedWordSymbolProvider(private val base: RustSymbolProvider, private val model: Model) :
    WrappingSymbolProvider(base) {
    private val internal =
        ReservedWordSymbolProvider.builder().symbolProvider(base).memberReservedWords(RustReservedWords).build()

    override fun toMemberName(shape: MemberShape): String {
        val baseName = internal.toMemberName(shape)
        return when (val container = model.expectShape(shape.container)) {
            is StructureShape -> when (baseName) {
                "build" -> "build_value"
                "builder" -> "builder_value"
                "default" -> "default_value"
                "send" -> "send_value"
                // To avoid conflicts with the `make_operation` and `presigned` functions on generated inputs
                "make_operation" -> "make_operation_value"
                "presigned" -> "presigned_value"
                else -> baseName
            }
            is UnionShape -> when (baseName) {
                // Unions contain an `Unknown` variant. This exists to support parsing data returned from the server
                // that represent union variants that have been added since this SDK was generated.
                UnionGenerator.UnknownVariantName -> "${UnionGenerator.UnknownVariantName}Value"
                "${UnionGenerator.UnknownVariantName}Value" -> "${UnionGenerator.UnknownVariantName}Value_"
                // Self cannot be used as a raw identifier, so we can't use the normal escaping strategy
                // https://internals.rust-lang.org/t/raw-identifiers-dont-work-for-all-identifiers/9094/4
                "Self" -> "SelfValue"
                // Real models won't end in `_` so it's safe to stop here
                "SelfValue" -> "SelfValue_"
                else -> baseName
            }
            else -> error("unexpected container: $container")
        }
    }

    /**
     * Convert shape to a Symbol
     *
     * If this symbol provider renamed the symbol, a `renamedFrom` field will be set on the symbol, enabling
     * code generators to generate special docs.
     */
    override fun toSymbol(shape: Shape): Symbol {
        return when (shape) {
            is MemberShape -> {
                val container = model.expectShape(shape.container)
                if (!(container is StructureShape || container is UnionShape)) {
                    return base.toSymbol(shape)
                }
                val previousName = base.toMemberName(shape)
                val escapedName = this.toMemberName(shape)
                val baseSymbol = base.toSymbol(shape)
                // if the names don't match and it isn't a simple escaping with `r#`, record a rename
                baseSymbol.letIf(escapedName != previousName && !escapedName.contains("r#")) {
                    it.toBuilder().renamedFrom(previousName).build()
                }
            }
            else -> base.toSymbol(shape)
        }
    }

    override fun toEnumVariantName(definition: EnumDefinition): MaybeRenamed? {
        val baseName = base.toEnumVariantName(definition) ?: return null
        check(definition.name.orNull()?.toPascalCase() == baseName.name) {
            "Enum variants must already be in pascal case ${baseName.name} differed from ${baseName.name.toPascalCase()}. Definition: ${definition.name}"
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

    fun escapeIfNeeded(word: String): String = when (isReserved(word)) {
        true -> escape(word)
        else -> word
    }

    override fun isReserved(word: String): Boolean = RustKeywords.contains(word)
}
