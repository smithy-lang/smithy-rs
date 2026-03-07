/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext

/**
 * A [StructureCustomization] that generates a `Schema` trait implementation
 * alongside the structure definition.
 *
 * @param extraConstructFields returns extra field initializers for the deserialize
 *   constructor based on the shape (e.g. `listOf("_request_id: None,")` for output shapes).
 *   Other decorators can add fields via [withExtraConstructField].
 */
class SchemaStructureCustomization(
    private val codegenContext: CodegenContext,
    private val extraConstructFields: (StructureShape) -> List<String> = { emptyList() },
) : StructureCustomization() {
    /**
     * Returns a new customization that adds an extra field initializer (e.g. `_fieldName: None,`)
     * for shapes matching the given predicate. This allows decorators that add extra struct fields
     * (like BaseRequestIdDecorator) to ensure the schema deserialize constructor includes them.
     */
    fun withExtraConstructField(
        fieldName: String,
        predicate: (StructureShape) -> Boolean,
    ): SchemaStructureCustomization =
        SchemaStructureCustomization(codegenContext) { shape ->
            val base = extraConstructFields(shape)
            if (predicate(shape)) base + "_$fieldName: None," else base
        }

    override fun section(section: StructureSection): Writable =
        when (section) {
            is StructureSection.AdditionalTraitImpls ->
                writable {
                    SchemaGenerator(
                        codegenContext,
                        this,
                        section.shape,
                        extraConstructFields = extraConstructFields(section.shape),
                    ).render()
                }
            else -> emptySection
        }
}
