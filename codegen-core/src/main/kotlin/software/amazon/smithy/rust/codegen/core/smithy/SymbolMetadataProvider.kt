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
import software.amazon.smithy.model.traits.EnumDefinition
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.SensitiveTrait
import software.amazon.smithy.model.traits.StreamingTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.util.hasTrait

/**
 * Default delegator to enable easily decorating another symbol provider.
 */
open class WrappingSymbolProvider(private val base: RustSymbolProvider) : RustSymbolProvider {
    override fun config(): SymbolVisitorConfig {
        return base.config()
    }

    override fun toEnumVariantName(definition: EnumDefinition): MaybeRenamed? {
        return base.toEnumVariantName(definition)
    }

    override fun toSymbol(shape: Shape): Symbol {
        return base.toSymbol(shape)
    }

    override fun toMemberName(shape: MemberShape): String {
        return base.toMemberName(shape)
    }
}

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
    val defaultDerives = setOf(RuntimeType.Debug, RuntimeType.PartialEq, RuntimeType.Clone)

    val isSensitive = shape.hasTrait<SensitiveTrait>() ||
        // Checking the shape's direct members for the sensitive trait should suffice.
        // Whether their descendants, i.e. a member's member, is sensitive does not
        // affect the inclusion/exclusion of the derived `Debug` trait of _this_ container
        // shape; any sensitive descendant should still be printed as redacted.
        shape.members().any { it.getMemberTrait(model, SensitiveTrait::class.java).isPresent }

    val setOfDerives = if (isSensitive) {
        defaultDerives - RuntimeType.Debug
    } else {
        defaultDerives
    }
    return RustMetadata(
        setOfDerives,
        additionalAttributes,
        Visibility.PUBLIC,
    )
}

/**
 * The base metadata supports a set of attributes that are used by generators to decorate code.
 *
 * By default we apply `#[non_exhaustive]` in [additionalAttributes] only to client structures since breaking model
 * changes are fine when generating server code.
 */
class BaseSymbolMetadataProvider(
    base: RustSymbolProvider,
    private val model: Model,
    private val additionalAttributes: List<Attribute>,
) : SymbolMetadataProvider(base) {

    override fun memberMeta(memberShape: MemberShape): RustMetadata =
        when (val container = model.expectShape(memberShape.container)) {
            is StructureShape -> {
                // TODO(https://github.com/awslabs/smithy-rs/issues/943): Once streaming accessors are usable,
                // then also make streaming members `#[doc(hidden)]`
                if (memberShape.getMemberTrait(model, StreamingTrait::class.java).isPresent) {
                    RustMetadata(visibility = Visibility.PUBLIC)
                } else {
                    RustMetadata(
                        // At some point, visibility _may_ be made `PRIVATE`, so make these `#[doc(hidden)]` for now.
                        visibility = Visibility.PUBLIC,
                        additionalAttributes = listOf(Attribute.DocHidden),
                    )
                }
            }

            is UnionShape, is CollectionShape, is MapShape -> RustMetadata(visibility = Visibility.PUBLIC)
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
    private val defaultRustMetadata = RustMetadata(visibility = Visibility.PRIVATE)

    override fun listMeta(listShape: ListShape) = defaultRustMetadata
    override fun mapMeta(mapShape: MapShape) = defaultRustMetadata
    override fun stringMeta(stringShape: StringShape) = defaultRustMetadata
    override fun numberMeta(numberShape: NumberShape) = defaultRustMetadata
    override fun blobMeta(blobShape: BlobShape) = defaultRustMetadata
}

private const val META_KEY = "meta"
fun Symbol.Builder.meta(rustMetadata: RustMetadata?): Symbol.Builder = this.putProperty(META_KEY, rustMetadata)

fun Symbol.expectRustMetadata(): RustMetadata = this.getProperty(META_KEY, RustMetadata::class.java).orElseThrow {
    CodegenException(
        "Expected `$this` to have metadata attached but it did not.",
    )
}
