/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProviderConfig
import software.amazon.smithy.rust.codegen.core.smithy.SymbolMetadataProvider
import software.amazon.smithy.rust.codegen.core.smithy.SymbolVisitor
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.core.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.isStreaming
import software.amazon.smithy.rust.codegen.server.smithy.ConstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustSettings
import java.util.logging.Logger

// Returns the Python implementation of the ByteStream shape or the original symbol that is provided in input.
private fun toPythonByteStreamSymbolOrOriginal(
    model: Model,
    config: RustSymbolProviderConfig,
    initial: Symbol,
    shape: Shape,
): Symbol {
    if (shape !is MemberShape) {
        return initial
    }

    val target = model.expectShape(shape.target)
    val container = model.expectShape(shape.container)

    if (!container.hasTrait<SyntheticOutputTrait>() && !container.hasTrait<SyntheticInputTrait>()) {
        return initial
    }

    // We are only targeting streaming blobs as the rest of the symbols do not change if streaming is enabled.
    // For example a TimestampShape doesn't become a different symbol when streaming is involved, but BlobShape
    // become a ByteStream.
    return if (target is BlobShape && shape.isStreaming(model)) {
        PythonServerRuntimeType.byteStream(config.runtimeConfig).toSymbol()
    } else {
        initial
    }
}

/**
 * Symbol provider that recursively replace symbols in nested shapes.
 *
 * Input / output / error structures can refer to complex types like the ones implemented inside
 * `aws_smithy_types` (a good example is `aws_smithy_types::Blob`).
 * `aws_smithy_http_server_python::types` wraps those types that do not implement directly the
 * `pyo3::PyClass` trait and cannot be shared safely with Python, providing an idiomatic Python / Rust API.
 *
 * This symbol provider ensures types not implementing `pyo3::PyClass` are swapped with their wrappers from
 * `aws_smithy_http_server_python::types`.
 */
class PythonServerSymbolVisitor(
    settings: ServerRustSettings,
    model: Model,
    serviceShape: ServiceShape?,
    config: RustSymbolProviderConfig,
) : SymbolVisitor(settings, model, serviceShape, config) {
    private val runtimeConfig = config.runtimeConfig
    private val logger = Logger.getLogger(javaClass.name)

    override fun toSymbol(shape: Shape): Symbol {
        val initial = shape.accept(this)
        return toPythonByteStreamSymbolOrOriginal(model, config, initial, shape)
    }

    override fun timestampShape(shape: TimestampShape?): Symbol {
        return PythonServerRuntimeType.dateTime(runtimeConfig).toSymbol()
    }

    override fun blobShape(shape: BlobShape?): Symbol {
        return PythonServerRuntimeType.blob(runtimeConfig).toSymbol()
    }

    override fun documentShape(shape: DocumentShape?): Symbol {
        return PythonServerRuntimeType.document(runtimeConfig).toSymbol()
    }
}

/**
 * Constrained symbol provider that recursively replace symbols in nested shapes.
 *
 * This symbol provider extends the `ConstrainedShapeSymbolProvider` to ensure constraints are
 * applied properly and swaps out shapes that do not implement `pyo3::PyClass` with their
 * wrappers.
 *
 * See `PythonServerSymbolVisitor` documentation for more info.
 */
class PythonConstrainedShapeSymbolProvider(
    base: RustSymbolProvider,
    serviceShape: ServiceShape,
    publicConstrainedTypes: Boolean,
) : ConstrainedShapeSymbolProvider(base, serviceShape, publicConstrainedTypes) {
    override fun toSymbol(shape: Shape): Symbol {
        val initial = super.toSymbol(shape)
        return toPythonByteStreamSymbolOrOriginal(model, config, initial, shape)
    }
}

/**
 * SymbolProvider to drop the PartialEq bounds in streaming shapes
 *
 * Streaming shapes equality cannot be checked without reading the body. Because of this, these shapes
 * do not implement `PartialEq`.
 *
 * Note that since streaming members can only be used on the root shape, this can only impact input and output shapes.
 */
class PythonStreamingShapeMetadataProvider(private val base: RustSymbolProvider) : SymbolMetadataProvider(base) {
    override fun structureMeta(structureShape: StructureShape): RustMetadata {
        val baseMetadata = base.toSymbol(structureShape).expectRustMetadata()
        return if (structureShape.hasStreamingMember(model)) {
            baseMetadata.withoutDerives(RuntimeType.PartialEq)
        } else {
            baseMetadata
        }
    }

    override fun unionMeta(unionShape: UnionShape): RustMetadata {
        val baseMetadata = base.toSymbol(unionShape).expectRustMetadata()
        return if (unionShape.hasStreamingMember(model)) {
            baseMetadata.withoutDerives(RuntimeType.PartialEq)
        } else {
            baseMetadata
        }
    }

    override fun memberMeta(memberShape: MemberShape) = base.toSymbol(memberShape).expectRustMetadata()

    override fun enumMeta(stringShape: StringShape) = base.toSymbol(stringShape).expectRustMetadata()

    override fun listMeta(listShape: ListShape) = base.toSymbol(listShape).expectRustMetadata()

    override fun mapMeta(mapShape: MapShape) = base.toSymbol(mapShape).expectRustMetadata()

    override fun stringMeta(stringShape: StringShape) = base.toSymbol(stringShape).expectRustMetadata()

    override fun numberMeta(numberShape: NumberShape) = base.toSymbol(numberShape).expectRustMetadata()

    override fun blobMeta(blobShape: BlobShape) = base.toSymbol(blobShape).expectRustMetadata()
}
