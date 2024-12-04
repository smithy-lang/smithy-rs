/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.client

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.smithy.customize.allCustomizationsAreEmpty
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations

class CustomizableOperationImplGenerator(
    private val codegenContext: ClientCodegenContext,
    private val operation: OperationShape,
    private val customizations: List<OperationCustomization>,
) {
    fun render(writer: RustWriter) {
        val section = OperationSection.CustomizableOperationImpl(customizations, operation)
        // When no customizations are set or there is nothing to write, return early.
        if (customizations.isEmpty() || allCustomizationsAreEmpty(customizations, section)) {
            return
        }

        operation.output
            .map { codegenContext.model.expectShape(it) }
            .map { codegenContext.symbolProvider.toSymbol(it) }
            .ifPresent {
                writer.rustBlock("impl<E, B> CustomizableOperation<#T, E, B>", it) {
                    writer.writeCustomizations(customizations, section)
                }
            }
    }
}
