package software.amazon.smithy.rust.codegen.smithy.symbol

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.traits.IdempotencyTokenTrait
import software.amazon.smithy.rust.codegen.smithy.RuntimeType

class IdempotencyTokenSymbolProvider(private val base: RustSymbolProvider) : WrappingSymbolProvider(base) {
    override fun toSymbol(shape: Shape): Symbol {
        val initial = base.toSymbol(shape)
        if (!shape.hasTrait(IdempotencyTokenTrait::class.java)) {
            return initial
        }
        check(shape is MemberShape)
        return initial.toBuilder().setDefault(
            Default.Custom {
                write("\$T(\$T())", RuntimeType.UuidV4(base.config().runtimeConfig), RuntimeType.Random)
            }
        ).build()
    }
}
