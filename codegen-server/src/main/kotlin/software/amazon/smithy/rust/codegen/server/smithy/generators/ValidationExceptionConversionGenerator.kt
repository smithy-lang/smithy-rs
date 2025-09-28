/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol

/**
 * Collection of methods that will be invoked by the respective generators to generate code to convert constraint
 * violations to validation exceptions.
 * This is only rendered for shapes that lie in a constrained operation's closure.
 */
interface ValidationExceptionConversionGenerator {
    val shapeId: ShapeId

    /**
     * Convert from a top-level operation input's constraint violation into
     * `aws_smithy_http_server::rejection::RequestRejection`.
     */
    fun renderImplFromConstraintViolationForRequestRejection(protocol: ServerProtocol): Writable

    // Simple shapes.
    fun stringShapeConstraintViolationImplBlock(stringConstraintsInfo: Collection<StringTraitInfo>): Writable

    fun enumShapeConstraintViolationImplBlock(enumTrait: EnumTrait): Writable

    fun numberShapeConstraintViolationImplBlock(rangeInfo: Range): Writable

    fun blobShapeConstraintViolationImplBlock(blobConstraintsInfo: Collection<BlobLength>): Writable

    // Aggregate shapes.
    fun mapShapeConstraintViolationImplBlock(
        shape: MapShape,
        keyShape: StringShape,
        valueShape: Shape,
        symbolProvider: RustSymbolProvider,
        model: Model,
    ): Writable

    fun builderConstraintViolationFn(constraintViolations: Collection<ConstraintViolation>): Writable

    fun collectionShapeConstraintViolationImplBlock(
        collectionConstraintsInfo: Collection<CollectionTraitInfo>,
        isMemberConstrained: Boolean,
    ): Writable

    fun unionShapeConstraintViolationImplBlock(
        unionConstraintTraitInfo: Collection<UnionConstraintTraitInfo>,
    ): Writable
}
