/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.StreamingTrait
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticOutputTrait

/**
 * Wrapping symbol provider to change `Blob` to `ByteStream` when it targets a streaming member
 */
class StreamingShapeSymbolProvider(private val base: RustSymbolProvider, private val model: Model) :
    WrappingSymbolProvider(base) {
    override fun toSymbol(shape: Shape): Symbol {
        val initial = base.toSymbol(shape)
        // We are only targetting member shapes
        if (shape !is MemberShape) {
            return initial
        }
        val target = model.expectShape(shape.target)
        val container = model.expectShape(shape.container)

        // We are only targeting output shapes
        if (!container.hasTrait(SyntheticOutputTrait::class.java)) {
            return initial
        }

        // We are only targeting streaming blobs
        return if (target is BlobShape && target.hasTrait(StreamingTrait::class.java)) {
            RuntimeType.byteStream(config().runtimeConfig).toSymbol()
        } else {
            base.toSymbol(shape)
        }
    }
}

fun StructureShape.hasStreamingMember(model: Model) = this.streamingMember(model) != null

fun StructureShape.streamingMember(model: Model): MemberShape? {
    return this.members().find { it.getMemberTrait(model, StreamingTrait::class.java).isPresent }
}

fun UnionShape.hasStreamingMember(model: Model): Boolean {
    return this.members().any { it.getMemberTrait(model, StreamingTrait::class.java).isPresent }
}

class StreamingShapeMetadataProvider(private val base: RustSymbolProvider, private val model: Model) : SymbolMetadataProvider(base) {
    override fun memberMeta(memberShape: MemberShape): RustMetadata {
        return base.toSymbol(memberShape).expectRustMetadata()
    }

    override fun structureMeta(structureShape: StructureShape): RustMetadata {
        val baseMetadata = base.toSymbol(structureShape).expectRustMetadata()
        return if (structureShape.hasStreamingMember(model)) {
            val withoutClone = baseMetadata.derives.copy(derives = baseMetadata.derives.derives - RuntimeType.Clone)
            baseMetadata.copy(derives = withoutClone)
        } else baseMetadata
    }

    override fun unionMeta(unionShape: UnionShape): RustMetadata {
        val baseMetadata = base.toSymbol(unionShape).expectRustMetadata()
        return if (unionShape.hasStreamingMember(model)) {
            val withoutClone = baseMetadata.derives.copy(derives = baseMetadata.derives.derives - RuntimeType.Clone)
            baseMetadata.copy(derives = withoutClone)
        } else baseMetadata
    }

    override fun enumMeta(stringShape: StringShape): RustMetadata {
        return base.toSymbol(stringShape).expectRustMetadata()
    }
}
