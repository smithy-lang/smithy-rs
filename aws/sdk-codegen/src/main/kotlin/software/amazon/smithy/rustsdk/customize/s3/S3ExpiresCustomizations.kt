/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.s3

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.ShapeType
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.DeprecatedTrait
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.model.traits.HttpHeaderTrait
import software.amazon.smithy.model.traits.OutputTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import kotlin.streams.asSequence

/**
 * Enforces that Expires fields have the DateTime type (since in the future the model will change to model them as String),
 * and add an ExpiresString field to maintain the raw string value sent.
 */
class S3ExpiresCustomizations {
    private val expires = "Expires"
    private val expiresString = "ExpiresString"

    fun processModel(model: Model): Model {
        val transformer = ModelTransformer.create()

        // Ensure all `Expires` shapes are timestamps
        val expiresShapeTimestampMap =
            model.shapes()
                .asSequence()
                .mapNotNull { shape ->
                    shape.members()
                        .singleOrNull { member -> member.memberName.equals("Expires", ignoreCase = true) }
                        ?.target
                }
                .associateWith { ShapeType.TIMESTAMP }
        var transformedModel = transformer.changeShapeType(model, expiresShapeTimestampMap)

        // Add an `ExpiresString` string shape to the model
        val expiresStringShape = StringShape.builder().id("aws.sdk.rust.s3.synthetic#$expiresString").build()
        transformedModel = transformedModel.toBuilder().addShape(expiresStringShape).build()

        // For output shapes only, deprecate `Expires` and add a synthetic member that targets `ExpiresString`
        transformedModel =
            transformer.mapShapes(transformedModel) { shape ->
                if (shape.hasTrait<OutputTrait>() && shape.memberNames.any { it.equals(expires, ignoreCase = true) }) {
                    val builder = (shape as StructureShape).toBuilder()

                    // Deprecate `Expires`
                    val expiresMember = shape.members().single { it.memberName.equals(expires, ignoreCase = true) }

                    builder.removeMember(expiresMember.memberName)
                    val deprecatedTrait =
                        DeprecatedTrait.builder()
                            .message("Please use `expires_string` which contains the raw, unparsed value of this field.")
                            .build()

                    builder.addMember(
                        expiresMember.toBuilder()
                            .addTrait(deprecatedTrait)
                            .build(),
                    )

                    // Add a synthetic member targeting `ExpiresString`
                    val expiresStringMember = MemberShape.builder()
                    expiresStringMember.target(expiresStringShape.id)
                    expiresStringMember.id(expiresMember.id.toString() + "String") // i.e. com.amazonaws.s3.<MEMBER_NAME>$ExpiresString
                    expiresStringMember.addTrait(HttpHeaderTrait(expiresString)) // Add HttpHeaderTrait to ensure the field is deserialized
                    expiresMember.getTrait<DocumentationTrait>()?.let {
                        expiresStringMember.addTrait(it) // Copy documentation from `Expires`
                    }
                    builder.addMember(expiresStringMember.build())
                    builder.build()
                } else {
                    shape
                }
            }

        return transformedModel
    }
}
