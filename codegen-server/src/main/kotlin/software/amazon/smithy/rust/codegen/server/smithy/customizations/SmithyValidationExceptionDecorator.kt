/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.generators.BlobLength
import software.amazon.smithy.rust.codegen.server.smithy.generators.CollectionTraitInfo
import software.amazon.smithy.rust.codegen.server.smithy.generators.ConstraintViolation
import software.amazon.smithy.rust.codegen.server.smithy.generators.Range
import software.amazon.smithy.rust.codegen.server.smithy.generators.StringTraitInfo
import software.amazon.smithy.rust.codegen.server.smithy.generators.TraitInfo
import software.amazon.smithy.rust.codegen.server.smithy.generators.UnionConstraintTraitInfo
import software.amazon.smithy.rust.codegen.server.smithy.generators.ValidationExceptionConversionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.isKeyConstrained
import software.amazon.smithy.rust.codegen.server.smithy.generators.isValueConstrained
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.validationErrorMessage

/**
 * A decorator that adds code to convert from constraint violations to Smithy's `smithy.framework#ValidationException`,
 * defined in [0]. This is Smithy's recommended shape to return when validation fails.
 *
 * This decorator is always enabled when using the `rust-server-codegen` plugin.
 *
 * [0]: https://github.com/awslabs/smithy/tree/main/smithy-validation-model
 *
 * TODO(https://github.com/smithy-lang/smithy-rs/pull/2053): once the RFC is implemented, consider moving this back into the
 *  generators.
 */
class SmithyValidationExceptionDecorator : ServerCodegenDecorator {
    override val name: String
        get() = "SmithyValidationExceptionDecorator"
    override val order: Byte
        get() = 69

    override fun validationExceptionConversion(
        codegenContext: ServerCodegenContext,
    ): ValidationExceptionConversionGenerator = SmithyValidationExceptionConversionGenerator(codegenContext)
}

class SmithyValidationExceptionConversionGenerator(private val codegenContext: ServerCodegenContext) :
    ValidationExceptionConversionGenerator {
    // Define a companion object so that we can refer to this shape id globally.
    companion object {
        val SHAPE_ID: ShapeId = ShapeId.from("smithy.framework#ValidationException")
    }

    override val shapeId: ShapeId = SHAPE_ID
    private val fieldGenerator = ValidationExceptionFieldGenerator(codegenContext, "ValidationExceptionField")

    override fun renderImplFromConstraintViolationForRequestRejection(protocol: ServerProtocol): Writable =
        fieldGenerator.renderImplFromConstraintViolationForRequestRejection(protocol)

    override fun stringShapeConstraintViolationImplBlock(stringConstraintsInfo: Collection<StringTraitInfo>): Writable =
        fieldGenerator.stringShapeConstraintViolationImplBlock(stringConstraintsInfo)

    override fun blobShapeConstraintViolationImplBlock(blobConstraintsInfo: Collection<BlobLength>): Writable =
        fieldGenerator.blobShapeConstraintViolationImplBlock(blobConstraintsInfo)

    override fun mapShapeConstraintViolationImplBlock(
        shape: MapShape,
        keyShape: StringShape,
        valueShape: Shape,
        symbolProvider: RustSymbolProvider,
        model: Model,
    ) = fieldGenerator.mapShapeConstraintViolationImplBlock(shape, keyShape, valueShape, symbolProvider, model)

    override fun enumShapeConstraintViolationImplBlock(enumTrait: EnumTrait) =
        fieldGenerator.enumShapeConstraintViolationImplBlock(enumTrait)

    override fun numberShapeConstraintViolationImplBlock(rangeInfo: Range) =
        fieldGenerator.numberShapeConstraintViolationImplBlock(rangeInfo)

    override fun builderConstraintViolationFn(constraintViolations: Collection<ConstraintViolation>) =
        fieldGenerator.builderConstraintViolationFn(constraintViolations)

    override fun collectionShapeConstraintViolationImplBlock(
        collectionConstraintsInfo: Collection<CollectionTraitInfo>,
        isMemberConstrained: Boolean,
    ) = fieldGenerator.collectionShapeConstraintViolationImplBlock(collectionConstraintsInfo, isMemberConstrained)

    override fun unionShapeConstraintViolationImplBlock(
        unionConstraintTraitInfo: Collection<UnionConstraintTraitInfo>,
    ) = fieldGenerator.unionShapeConstraintViolationImplBlock(unionConstraintTraitInfo)
}
