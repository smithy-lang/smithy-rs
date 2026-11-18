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
 */
class SchemaStructureCustomization(
    private val codegenContext: CodegenContext,
) : StructureCustomization() {
    override fun section(section: StructureSection): Writable =
        when (section) {
            is StructureSection.AdditionalTraitImpls ->
                writable {
                    SchemaGenerator(codegenContext, this, section.shape).renderInOwnModule()
                }
            else -> emptySection
        }
}
