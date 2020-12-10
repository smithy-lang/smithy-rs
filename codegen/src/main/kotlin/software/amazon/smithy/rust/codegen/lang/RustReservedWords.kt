package software.amazon.smithy.rust.codegen.lang

import software.amazon.smithy.codegen.core.ReservedWordSymbolProvider
import software.amazon.smithy.codegen.core.ReservedWords
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.WrappingSymbolProvider

class RustReservedWordSymbolProvider(base: RustSymbolProvider) : WrappingSymbolProvider(base) {
    private val internal = ReservedWordSymbolProvider.builder().symbolProvider(base).memberReservedWords(RustReservedWords).build()
    override fun toMemberName(shape: MemberShape): String {
        return internal.toMemberName(shape)
    }

    override fun toSymbol(shape: Shape): Symbol {
        return internal.toSymbol(shape)
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

    override fun escape(word: String): String = "r##$word"

    override fun isReserved(word: String): Boolean = RustKeywords.contains(word)
}
