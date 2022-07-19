/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
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
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.isStreaming

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
        if (!(container.hasTrait<SyntheticOutputTrait>() || container.hasTrait<SyntheticInputTrait>())) {
            return initial
        }

        // We are only targeting streaming blobs
        return if (target is BlobShape && shape.isStreaming(model)) {
            RuntimeType.ByteStream(config().runtimeConfig).toSymbol().toBuilder().setDefault(Default.RustDefault).build()
        } else {
            base.toSymbol(shape)
        }
    }
}

/**
 * SymbolProvider to drop the clone and PartialEq bounds in streaming shapes
 *
 * Streaming shapes cannot be cloned and equality cannot be checked without reading the body. Because of this, these shapes
 * do not implement `Clone` or `PartialEq`.
 *
 * Note that since streaming members can only be used on the root shape, this can only impact input and output shapes.
 */
class StreamingShapeMetadataProvider(private val base: RustSymbolProvider, private val model: Model) : SymbolMetadataProvider(base) {
    override fun memberMeta(memberShape: MemberShape): RustMetadata {
        return base.toSymbol(memberShape).expectRustMetadata()
    }

    override fun structureMeta(structureShape: StructureShape): RustMetadata {
        val baseMetadata = base.toSymbol(structureShape).expectRustMetadata()
        return if (structureShape.hasStreamingMember(model)) {
            baseMetadata.withoutDerives(RuntimeType.Clone, RuntimeType.PartialEq)
        } else baseMetadata
    }

    override fun unionMeta(unionShape: UnionShape): RustMetadata {
        val baseMetadata = base.toSymbol(unionShape).expectRustMetadata()
        return if (unionShape.hasStreamingMember(model)) {
            baseMetadata.withoutDerives(RuntimeType.Clone, RuntimeType.PartialEq)
        } else baseMetadata
    }

    override fun enumMeta(stringShape: StringShape): RustMetadata {
        return base.toSymbol(stringShape).expectRustMetadata()
    }
}
