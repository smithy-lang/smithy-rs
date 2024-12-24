/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.s3

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeType
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.DeprecatedTrait
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.model.traits.HttpHeaderTrait
import software.amazon.smithy.model.traits.OutputTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustSettings
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rustsdk.InlineAwsDependency
import kotlin.streams.asSequence

/**
 * Enforces that Expires fields have the DateTime type (since in the future the model will change to model them as String),
 * and add an ExpiresString field to maintain the raw string value sent.
 */
class S3ExpiresDecorator : ClientCodegenDecorator {
    override val name: String = "S3ExpiresDecorator"
    override val order: Byte = 0
    private val expires = "Expires"
    private val expiresString = "ExpiresString"

    override fun transformModel(
        service: ServiceShape,
        model: Model,
        settings: ClientRustSettings,
    ): Model {
        val transformer = ModelTransformer.create()

        // Ensure all `Expires` shapes are timestamps
        val expiresShapeTimestampMap =
            model.shapes()
                .asSequence()
                .mapNotNull { shape ->
                    shape.members()
                        .singleOrNull { member -> member.memberName.equals(expires, ignoreCase = true) }
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

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        val outputShape = operation.outputShape(codegenContext.model)

        if (outputShape.memberNames.any { it.equals(expires, ignoreCase = true) }) {
            return baseCustomizations +
                ParseExpiresFieldsCustomization(
                    codegenContext,
                )
        } else {
            return baseCustomizations
        }
    }
}

class ParseExpiresFieldsCustomization(
    private val codegenContext: ClientCodegenContext,
) : OperationCustomization() {
    override fun section(section: OperationSection): Writable =
        writable {
            when (section) {
                is OperationSection.AdditionalInterceptors -> {
                    section.registerInterceptor(codegenContext.runtimeConfig, this) {
                        val interceptor =
                            RuntimeType.forInlineDependency(
                                InlineAwsDependency.forRustFile("s3_expires_interceptor"),
                            ).resolve("S3ExpiresInterceptor")
                        rustTemplate(
                            """
                            #{S3ExpiresInterceptor}
                            """,
                            "S3ExpiresInterceptor" to interceptor,
                        )
                    }
                }

                else -> {}
            }
        }
}
