/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext

/**
 * A [StructureCustomization] that generates a `Schema` implementation for the
 * structure, in a module of its own (see [SchemaGenerator.renderInOwnModule]).
 *
 * [documentV2Companions] is passed to [SchemaGenerator]. Set it when the
 * structures have the companions that [DocumentV2Index] lists.
 */
class SchemaStructureCustomization(
    private val codegenContext: CodegenContext,
    private val documentV2Companions: Boolean = false,
) : StructureCustomization() {
    override fun section(section: StructureSection): Writable =
        when (section) {
            is StructureSection.AdditionalTraitImpls ->
                writable {
                    SchemaGenerator(
                        codegenContext,
                        this,
                        section.shape,
                        documentV2Companions = documentV2Companions,
                    ).renderInOwnModule()
                }
            else -> emptySection
        }
}
