package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.DoubleShape
import software.amazon.smithy.model.shapes.FloatShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.StreamingTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.core.smithy.DirectedWalker
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.SymbolMetadataProvider
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.util.hasTrait

// TODO Docs
// TODO Test
class DeriveEqAndHashSymbolMetadataProvider(
    private val base: RustSymbolProvider,
    val model: Model,
) : SymbolMetadataProvider(base) {
    private val walker = DirectedWalker(model)

    private fun addDeriveEqAndHashIfPossible(shape: Shape): RustMetadata {
        check(shape !is MemberShape)
        val baseMetadata = base.toSymbol(shape).expectRustMetadata()
        // TODO Justify
        return if (walker.walkShapes(shape)
            .any { it is FloatShape || it is DoubleShape || it is DocumentShape || it.hasTrait<StreamingTrait>() }
        ) {
            baseMetadata
        } else {
            var ret = baseMetadata
            if (ret.derives.contains(RuntimeType.PartialEq)) {
                // We can only derive `Eq` if the type implements `PartialEq`. Not every shape that does not reach a
                // floating point or a document shape does; for example, streaming shapes cannot be `PartialEq`, see
                // [StreamingShapeMetadataProvider].
                ret = ret.withDerives(RuntimeType.Eq)
            }

            // `std::collections::HashMap` does not implement `std::hash::Hash`:
            // https://github.com/awslabs/smithy/issues/1567
            if (walker.walkShapes(shape).none { it is MapShape }) {
                ret = ret.withDerives(RuntimeType.Hash)
            }

            return ret
        }
    }

    override fun memberMeta(memberShape: MemberShape) = base.toSymbol(memberShape).expectRustMetadata()

    override fun structureMeta(structureShape: StructureShape) = addDeriveEqAndHashIfPossible(structureShape)
    override fun unionMeta(unionShape: UnionShape) = addDeriveEqAndHashIfPossible(unionShape)
    override fun enumMeta(stringShape: StringShape) = addDeriveEqAndHashIfPossible(stringShape)

    override fun listMeta(listShape: ListShape): RustMetadata = addDeriveEqAndHashIfPossible(listShape)
    override fun mapMeta(mapShape: MapShape): RustMetadata = addDeriveEqAndHashIfPossible(mapShape)
    override fun stringMeta(stringShape: StringShape): RustMetadata = addDeriveEqAndHashIfPossible(stringShape)
    override fun numberMeta(numberShape: NumberShape): RustMetadata = addDeriveEqAndHashIfPossible(numberShape)
}
