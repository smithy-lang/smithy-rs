/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.smithy.generators.SchemaStructureCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureCustomization
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait

/**
 * Generates Schema implementations for all structure shapes, enabling
 * protocol-agnostic serialization and deserialization.
 */
class SchemaDecorator : ClientCodegenDecorator {
    override val name: String = "SchemaDecorator"
    override val order: Byte = 0

    // Uncomment the following to limit schema generation to specific services
    // during phased rollout. When the list is empty or this is commented out,
    // schemas are generated for all services.
    //
    // private val allowedServices = setOf(
    //     "com.amazonaws.dynamodb#DynamoDB_20120810",
    //     "com.amazonaws.sqs#AmazonSQS",
    //     "com.amazonaws.s3#AmazonS3",
    // )
    //
    // private fun isEnabled(codegenContext: ClientCodegenContext): Boolean =
    //     allowedServices.contains(codegenContext.serviceShape.id.toString())

    override fun structureCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<StructureCustomization>,
    ): List<StructureCustomization> {
        // if (!isEnabled(codegenContext)) return baseCustomizations

        // Output shapes have extra fields added by BaseRequestIdDecorator subclasses.
        // Collect the field names that need default initialization in the deserialize constructor.
        val extraOutputFields = mutableListOf("_request_id: None,")
        // S3 adds _extended_request_id via S3ExtendedRequestIdDecorator
        if (codegenContext.serviceShape.id.name.startsWith("AmazonS3")) {
            extraOutputFields.add("_extended_request_id: None,")
        }

        return baseCustomizations +
            SchemaStructureCustomization(codegenContext) { shape ->
                if (shape.hasTrait(SyntheticOutputTrait::class.java)) {
                    extraOutputFields
                } else {
                    emptyList()
                }
            }
    }
}
