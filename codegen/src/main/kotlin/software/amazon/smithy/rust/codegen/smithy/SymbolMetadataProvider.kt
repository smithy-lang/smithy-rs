package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.lang.Derives
import software.amazon.smithy.rust.codegen.lang.RustMetadata

/**
 * Default delegator to enable easily decorating another symbol provider.
 */
open class WrappingSymbolProvider(private val base: RustSymbolProvider) : RustSymbolProvider {
    override fun config(): SymbolVisitorConfig {
        return base.config()
    }

    override fun toSymbol(shape: Shape): Symbol {
        return base.toSymbol(shape)
    }

    override fun toMemberName(shape: MemberShape): String {
        return base.toMemberName(shape)
    }
}

/**
 * Attach `meta` to symbols. `meta` is used by the generators (eg. StructureGenerator) to configure the generated models.
 *
 * Protocols may inherit from this class and override the `xyzMeta` methods to modify structure generation.
 */
abstract class SymbolMetadataProvider(private val base: RustSymbolProvider) : WrappingSymbolProvider(base) {
    override fun toSymbol(shape: Shape): Symbol {
        val baseSymbol = base.toSymbol(shape)
        val meta = when (shape) {
            is MemberShape -> memberMeta(shape)
            is StructureShape -> structureMeta(shape)
            is UnionShape -> unionMeta(shape)
            is StringShape -> if (shape.hasTrait(EnumTrait::class.java)) {
                enumMeta(shape)
            } else null
            else -> null
        }
        return baseSymbol.toBuilder().meta(meta).build()
    }

    abstract fun memberMeta(memberShape: MemberShape): RustMetadata
    abstract fun structureMeta(structureShape: StructureShape): RustMetadata
    abstract fun unionMeta(unionShape: UnionShape): RustMetadata
    abstract fun enumMeta(stringShape: StringShape): RustMetadata
}

class BaseSymbolMetadataProvider(base: RustSymbolProvider) : SymbolMetadataProvider(base) {
    override fun memberMeta(memberShape: MemberShape): RustMetadata {
        return RustMetadata(public = true)
    }

    override fun structureMeta(structureShape: StructureShape): RustMetadata {
        return RustMetadata(defaultDerives, public = true)
    }

    override fun unionMeta(unionShape: UnionShape): RustMetadata {
        return RustMetadata(defaultDerives, public = true)
    }

    override fun enumMeta(stringShape: StringShape): RustMetadata {
        return RustMetadata(
            Derives(
                defaultDerives.derives +
                    // enums must be hashable because string sets are hashable
                    RuntimeType.Std("hash::Hash") +
                    // enums can be eq because they can only contain strings
                    RuntimeType.Std("cmp::Eq") +
                    // enums can be Ord because they can only contain strings
                    RuntimeType.Std("cmp::PartialOrd") +
                    RuntimeType.Std("cmp::Ord")
            ),
            public = true
        )
    }

    companion object {
        val defaultDerives =
            Derives(setOf(RuntimeType.StdFmt("Debug"), RuntimeType.Std("cmp::PartialEq"), RuntimeType.Std("clone::Clone")))
    }
}

private const val MetaKey = "meta"
fun Symbol.Builder.meta(rustMetadata: RustMetadata?): Symbol.Builder {
    return this.putProperty(MetaKey, rustMetadata)
}
fun Symbol.expectRustMetadata(): RustMetadata = this.getProperty(MetaKey, RustMetadata::class.java).orElseThrow {
    CodegenException(
        "Expected $this to have metadata attached but it did not. "
    )
}
