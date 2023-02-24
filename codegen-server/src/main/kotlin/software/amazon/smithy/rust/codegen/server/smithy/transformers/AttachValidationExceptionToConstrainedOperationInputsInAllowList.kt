/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.transformers

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.EnumShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.core.smithy.DirectedWalker
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.server.smithy.customizations.SmithyValidationExceptionConversionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.hasConstraintTrait

/**
 * Attach the `smithy.framework#ValidationException` error to operations whose inputs are constrained, if they belong
 * to a service in an allowlist.
 *
 * Some of the models we generate in CI have constrained operation inputs, but the operations don't have
 * `smithy.framework#ValidationException` in their list of errors. This is a codegen error, unless
 * `disableDefaultValidation` is set to `true`, a code generation mode we don't support yet. See [1] for more details.
 * Until we implement said mode, we manually attach the error to build these models, since we don't own them (they're
 * either actual AWS service model excerpts, or they come from the `awslabs/smithy` library.
 *
 * [1]: https://github.com/awslabs/smithy-rs/pull/1199#discussion_r809424783
 *
 * TODO(https://github.com/awslabs/smithy-rs/issues/1401): This transformer will go away once we add support for
 *  `disableDefaultValidation` set to `true`, allowing service owners to map from constraint violations to operation errors.
 */
object AttachValidationExceptionToConstrainedOperationInputsInAllowList {
    private val sherviceShapeIdAllowList =
        setOf(
            // These we currently generate server SDKs for.
            ShapeId.from("aws.protocoltests.restjson#RestJson"),
            ShapeId.from("aws.protocoltests.json10#JsonRpc10"),
            ShapeId.from("aws.protocoltests.json#JsonProtocol"),
            ShapeId.from("com.amazonaws.s3#AmazonS3"),
            ShapeId.from("com.amazonaws.ebs#Ebs"),

            // These are only loaded in the classpath and need this model transformer, but we don't generate server
            // SDKs for them. Here they are for reference.
            // ShapeId.from("aws.protocoltests.restxml#RestXml"),
            // ShapeId.from("com.amazonaws.glacier#Glacier"),
            // ShapeId.from("aws.protocoltests.ec2#AwsEc2"),
            // ShapeId.from("aws.protocoltests.query#AwsQuery"),
            // ShapeId.from("com.amazonaws.machinelearning#AmazonML_20141212"),
        )

    fun transform(model: Model): Model {
        val walker = DirectedWalker(model)
        val operationsWithConstrainedInputWithoutValidationException = model.serviceShapes
            .filter { sherviceShapeIdAllowList.contains(it.toShapeId()) }
            .flatMap { it.operations }
            .map { model.expectShape(it, OperationShape::class.java) }
            .filter { operationShape ->
                // Walk the shapes reachable via this operation input.
                walker.walkShapes(operationShape.inputShape(model))
                    .any { it is SetShape || it is EnumShape || it.hasConstraintTrait() }
            }
            .filter { !it.errors.contains(SmithyValidationExceptionConversionGenerator.SHAPE_ID) }

        return ModelTransformer.create().mapShapes(model) { shape ->
            if (shape is OperationShape && operationsWithConstrainedInputWithoutValidationException.contains(shape)) {
                shape.toBuilder().addError(SmithyValidationExceptionConversionGenerator.SHAPE_ID).build()
            } else {
                shape
            }
        }
    }
}
