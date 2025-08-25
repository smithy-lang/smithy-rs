/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.typescript.smithy

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
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
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
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustSettings
import java.util.logging.Logger

/**
 * Symbol visitor  allowing that recursively replace symbols in nested shapes.
 *
 * Input / output / error structures can refer to complex types like the ones implemented inside
 * `aws_smithy_types` (a good example is `aws_smithy_types::Blob`).
 * `aws_smithy_http_server_typescript::types` wraps those types that do not implement directly the
 * `napi` trait and cannot be shared safely with Typescript, providing an idiomatic Typescript / Rust API.
 *
 * This symbol provider ensures types not implementing `pyo3::PyClass` are swapped with their wrappers from
 * `aws_smithy_http_server_typescript::types`.
 */
class TsServerSymbolVisitor(
    settings: ServerRustSettings,
    model: Model,
    serviceShape: ServiceShape?,
    config: RustSymbolProviderConfig,
) : SymbolVisitor(settings, model, serviceShape, config) {
    private val runtimeConfig = config.runtimeConfig
    private val logger = Logger.getLogger(javaClass.name)

    override fun toSymbol(shape: Shape): Symbol {
        val initial = shape.accept(this)

        if (shape !is MemberShape) {
            return initial
        }
        val target = model.expectShape(shape.target)
        val container = model.expectShape(shape.container)

        // We are only targeting non-synthetic inputs and outputs.
        if (!container.hasTrait<SyntheticOutputTrait>() && !container.hasTrait<SyntheticInputTrait>()) {
            return initial
        }

        // We are only targeting streaming blobs as the rest of the symbols do not change if streaming is enabled.
        // For example a TimestampShape doesn't become a different symbol when streaming is involved, but BlobShape
        // become a ByteStream.
        return if (target is BlobShape && shape.isStreaming(model)) {
            TsServerRuntimeType.byteStream(runtimeConfig).toSymbol()
        } else {
            initial
        }
    }

    override fun timestampShape(shape: TimestampShape?): Symbol {
        return TsServerRuntimeType.dateTime(runtimeConfig).toSymbol()
    }

    override fun blobShape(shape: BlobShape?): Symbol {
        return TsServerRuntimeType.blob(runtimeConfig).toSymbol()
    }

    override fun documentShape(shape: DocumentShape?): Symbol {
        return TsServerRuntimeType.document(runtimeConfig).toSymbol()
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
class TsStreamingShapeMetadataProvider(private val base: RustSymbolProvider) : SymbolMetadataProvider(base) {
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

    override fun enumMeta(stringShape: StringShape): RustMetadata =
        RustMetadata(
            setOf(RuntimeType.Eq, RuntimeType.Ord, RuntimeType.PartialEq, RuntimeType.PartialOrd, RuntimeType.Debug),
            listOf(),
            Visibility.PUBLIC,
        )

    override fun listMeta(listShape: ListShape) = base.toSymbol(listShape).expectRustMetadata()

    override fun mapMeta(mapShape: MapShape) = base.toSymbol(mapShape).expectRustMetadata()

    override fun stringMeta(stringShape: StringShape) = base.toSymbol(stringShape).expectRustMetadata()

    override fun numberMeta(numberShape: NumberShape) = base.toSymbol(numberShape).expectRustMetadata()

    override fun blobMeta(blobShape: BlobShape) = base.toSymbol(blobShape).expectRustMetadata()
}
