/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.rustlang

import software.amazon.smithy.codegen.core.ReservedWordSymbolProvider
import software.amazon.smithy.codegen.core.ReservedWords
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.EnumShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.renamedFrom
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.letIf

data class RustReservedWordConfig(
    /** Map of struct member names that should get renamed */
    val structureMemberMap: Map<String, String>,
    /** Map of union member names that should get renamed */
    val unionMemberMap: Map<String, String>,
    /** Map of enum member names that should get renamed */
    val enumMemberMap: Map<String, String>,
)

class RustReservedWordSymbolProvider(
    private val base: RustSymbolProvider,
    private val reservedWordConfig: RustReservedWordConfig,
) : WrappingSymbolProvider(base) {
    private val internal =
        ReservedWordSymbolProvider.builder().symbolProvider(base)
            .nameReservedWords(RustReservedWords)
            .memberReservedWords(RustReservedWords)
            .build()

    override fun toMemberName(shape: MemberShape): String {
        val baseName = super.toMemberName(shape)
        val reservedWordReplacedName = internal.toMemberName(shape)
        val container = model.expectShape(shape.container)
        return when {
            container is StructureShape ->
                reservedWordConfig.structureMemberMap.getOrDefault(baseName, reservedWordReplacedName)

            container is UnionShape ->
                reservedWordConfig.unionMemberMap.getOrDefault(baseName, reservedWordReplacedName)

            container is EnumShape || container.hasTrait<EnumTrait>() ->
                reservedWordConfig.enumMemberMap.getOrDefault(baseName, reservedWordReplacedName)

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
        // Sanity check that the symbol provider stack is set up correctly
        check(super.toSymbol(shape).renamedFrom() == null) {
            "RustReservedWordSymbolProvider should only run once"
        }

        val renamedSymbol = internal.toSymbol(shape)
        return when (shape) {
            is MemberShape -> {
                val container = model.expectShape(shape.container)
                val containerIsEnum = container is EnumShape || container.hasTrait<EnumTrait>()
                if (container !is StructureShape && container !is UnionShape && !containerIsEnum) {
                    return base.toSymbol(shape)
                }
                val previousName = base.toMemberName(shape)
                // Prefix leading digit with an underscore to avoid invalid identifiers; allow extra leading underscores.
                val escapedName = this.toMemberName(shape).replace(Regex("^(_*\\d)"), "_$1")
                // if the names don't match and it isn't a simple escaping with `r#`, record a rename
                renamedSymbol.toBuilder().name(escapedName)
                    .letIf(escapedName != previousName && !escapedName.contains("r#")) {
                        it.renamedFrom(previousName)
                    }.build()
            }

            else -> renamedSymbol
        }
    }
}

enum class EscapeFor {
    TypeName,
    ModuleName,
}

object RustReservedWords : ReservedWords {
    // This is the same list defined in `CodegenTestCommon` from the `buildSrc` Gradle subproject.
    private val RustKeywords =
        setOf(
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
            "try",
        )

    // Some things can't be used as a raw identifier, so we can't use the normal escaping strategy
    // https://internals.rust-lang.org/t/raw-identifiers-dont-work-for-all-identifiers/9094/4
    private val keywordEscapingMap =
        mapOf(
            "crate" to "crate_",
            "super" to "super_",
            "self" to "self_",
            "Self" to "SelfValue",
            // Real models won't end in `_` so it's safe to stop here
            "SelfValue" to "SelfValue_",
        )

    override fun escape(word: String): String = doEscape(word, EscapeFor.TypeName)

    private fun doEscape(
        word: String,
        escapeFor: EscapeFor = EscapeFor.TypeName,
    ): String =
        when (val mapped = keywordEscapingMap[word]) {
            null ->
                when (escapeFor) {
                    EscapeFor.TypeName -> "r##$word"
                    EscapeFor.ModuleName -> "${word}_"
                }
            else -> mapped
        }

    fun escapeIfNeeded(
        word: String,
        escapeFor: EscapeFor = EscapeFor.TypeName,
    ): String =
        when (isReserved(word)) {
            true -> doEscape(word, escapeFor)
            else -> word
        }

    override fun isReserved(word: String): Boolean = RustKeywords.contains(word) || keywordEscapingMap.contains(word)
}
