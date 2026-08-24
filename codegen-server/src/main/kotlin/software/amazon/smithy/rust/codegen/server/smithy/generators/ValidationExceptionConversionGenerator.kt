/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider

/**
 * Collection of methods that will be invoked by the respective generators to generate code to convert constraint
 * violations to validation exceptions.
 * This is only rendered for shapes that lie in a constrained operation's closure.
 */
interface ValidationExceptionConversionGenerator {
    val shapeId: ShapeId

    /**
     * The id of the validation error structure IN THE MODEL that this generator converts
     * constraint violations into. Usually [shapeId]; decorators whose [shapeId] is a
     * sentinel that is not itself a model shape (e.g. the user-provided validation
     * exception) override this with the real structure's id.
     */
    fun validationExceptionShapeId(): ShapeId = shapeId

    /**
     * The protocol-free conversion from a top-level operation input's constraint
     * violation into the modeled validation error shape (default
     * `smithy.framework#ValidationException` or a decorator-customized shape):
     * `impl From<ConstraintViolation> for {ValidationShape}`, building the value
     * (message, field list — frozen format strings). This is the ONLY place the
     * three validation decorators customize; serialization happens once, at the
     * protocol boundary, via `ServerProtocol::serialize_error`.
     */
    fun renderImplFromConstraintViolationForValidationException(
        constraintViolation: RuntimeType = RuntimeType("ConstraintViolation"),
    ): Writable

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
