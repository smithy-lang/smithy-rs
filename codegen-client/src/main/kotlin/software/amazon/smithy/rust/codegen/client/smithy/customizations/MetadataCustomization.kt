/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.sdkId

class MetadataCustomization(
    private val codegenContext: ClientCodegenContext,
    operation: OperationShape,
) : OperationCustomization() {
    private val operationName = codegenContext.symbolProvider.toSymbol(operation).name
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope by lazy {
        arrayOf(
            "Metadata" to RuntimeType.operationModule(runtimeConfig).resolve("Metadata"),
        )
    }

    override fun section(section: OperationSection): Writable = writable {
        when (section) {
            is OperationSection.AdditionalRuntimePluginConfig -> {
                rustTemplate(
                    """
                    ${section.newLayerName}.store_put(#{Metadata}::new(
                        ${operationName.dq()},
                        ${codegenContext.serviceShape.sdkId().dq()},
                    ));
                    """,
                    *codegenScope,
                )
            }

            else -> {}
        }
    }
}
