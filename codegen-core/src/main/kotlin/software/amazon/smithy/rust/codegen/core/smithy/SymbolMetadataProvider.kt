/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.SensitiveTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.util.hasTrait

/**
 * Attach `meta` to symbols. `meta` is used by the generators (e.g. StructureGenerator) to configure the generated models.
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
            is ListShape -> listMeta(shape)
            is MapShape -> mapMeta(shape)
            is NumberShape -> numberMeta(shape)
            is BlobShape -> blobMeta(shape)
            is StringShape -> if (shape.hasTrait<EnumTrait>()) {
                enumMeta(shape)
            } else {
                stringMeta(shape)
            }

            else -> null
        }
        return baseSymbol.toBuilder().meta(meta).build()
    }

    abstract fun memberMeta(memberShape: MemberShape): RustMetadata
    abstract fun structureMeta(structureShape: StructureShape): RustMetadata
    abstract fun unionMeta(unionShape: UnionShape): RustMetadata
    abstract fun enumMeta(stringShape: StringShape): RustMetadata

    abstract fun listMeta(listShape: ListShape): RustMetadata
    abstract fun mapMeta(mapShape: MapShape): RustMetadata
    abstract fun stringMeta(stringShape: StringShape): RustMetadata
    abstract fun numberMeta(numberShape: NumberShape): RustMetadata
    abstract fun blobMeta(blobShape: BlobShape): RustMetadata
}

fun containerDefaultMetadata(
    shape: Shape,
    model: Model,
    additionalAttributes: List<Attribute> = emptyList(),
): RustMetadata {
    val derives = mutableSetOf(RuntimeType.Debug, RuntimeType.PartialEq, RuntimeType.Clone)

    val isSensitive = shape.hasTrait<SensitiveTrait>() ||
        // Checking the shape's direct members for the sensitive trait should suffice.
        // Whether their descendants, i.e. a member's member, is sensitive does not
        // affect the inclusion/exclusion of the derived `Debug` trait of _this_ container
        // shape; any sensitive descendant should still be printed as redacted.
        shape.members().any { it.getMemberTrait(model, SensitiveTrait::class.java).isPresent }

    if (isSensitive) {
        derives.remove(RuntimeType.Debug)
    }

    return RustMetadata(derives, additionalAttributes, Visibility.PUBLIC)
}

/**
 * The base metadata supports a set of attributes that are used by generators to decorate code.
 *
 * By default, we apply `#[non_exhaustive]` in [additionalAttributes] only to client structures since breaking model
 * changes are fine when generating server code.
 */
class BaseSymbolMetadataProvider(
    base: RustSymbolProvider,
    private val additionalAttributes: List<Attribute>,
) : SymbolMetadataProvider(base) {

    override fun memberMeta(memberShape: MemberShape): RustMetadata =
        when (val container = model.expectShape(memberShape.container)) {
            is StructureShape -> RustMetadata(visibility = Visibility.PUBLIC)

            is UnionShape, is CollectionShape, is MapShape -> RustMetadata(visibility = Visibility.PUBLIC)

            // This covers strings with the enum trait for now, and can be removed once we're fully on EnumShape
            // TODO(https://github.com/awslabs/smithy-rs/issues/1700): Remove this `is StringShape` match arm
            is StringShape -> RustMetadata(visibility = Visibility.PUBLIC)

            else -> TODO("Unrecognized container type: $container")
        }

    override fun structureMeta(structureShape: StructureShape) = containerDefaultMetadata(structureShape, model, additionalAttributes)
    override fun unionMeta(unionShape: UnionShape) = containerDefaultMetadata(unionShape, model, additionalAttributes)

    override fun enumMeta(stringShape: StringShape): RustMetadata =
        containerDefaultMetadata(stringShape, model, additionalAttributes).withDerives(
            // Smithy's `enum` shapes can additionally be `Eq`, `PartialOrd`, `Ord`, and `Hash` because they can
            // only contain strings.
            RuntimeType.Eq,
            RuntimeType.PartialOrd,
            RuntimeType.Ord,
            RuntimeType.Hash,
        )

    // Only the server subproject uses these, so we provide a sane and conservative default implementation here so that
    // the rest of symbol metadata providers can just delegate to it.
    private fun defaultRustMetadata() = RustMetadata(visibility = Visibility.PRIVATE)

    override fun listMeta(listShape: ListShape) = defaultRustMetadata()
    override fun mapMeta(mapShape: MapShape) = defaultRustMetadata()
    override fun stringMeta(stringShape: StringShape) = defaultRustMetadata()
    override fun numberMeta(numberShape: NumberShape) = defaultRustMetadata()
    override fun blobMeta(blobShape: BlobShape) = defaultRustMetadata()
}

private const val META_KEY = "meta"
fun Symbol.Builder.meta(rustMetadata: RustMetadata?): Symbol.Builder = this.putProperty(META_KEY, rustMetadata)

fun Symbol.expectRustMetadata(): RustMetadata = this.getProperty(META_KEY, RustMetadata::class.java).orElseThrow {
    CodegenException(
        "Expected `$this` to have metadata attached but it did not.",
    )
}
