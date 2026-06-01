/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext

/**
 * A [StructureCustomization] that generates a `Schema` implementation
 * alongside the structure definition.
 */
class SchemaStructureCustomization(
    private val codegenContext: CodegenContext,
    private val syntheticMembers: List<SyntheticSchemaMember> = emptyList(),
) : StructureCustomization() {
    override fun section(section: StructureSection): Writable =
        when (section) {
            is StructureSection.AdditionalTraitImpls ->
                writable {
                    val members =
                        if (section.shape.hasTrait(software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait::class.java)) {
                            syntheticMembers
                        } else {
                            emptyList()
                        }
                    SchemaGenerator(
                        codegenContext,
                        this,
                        section.shape,
                        syntheticMembers = members,
                    ).render()
                }
            else -> emptySection
        }
}
