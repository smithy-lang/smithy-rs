/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.SensitiveIndex
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

class SensitiveOutputDecorator : ClientCodegenDecorator {
    override val name: String get() = "SensitiveOutputDecorator"
    override val order: Byte get() = 0

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> =
        baseCustomizations + listOf(SensitiveOutputCustomization(codegenContext, operation))
}

private class SensitiveOutputCustomization(
    private val codegenContext: ClientCodegenContext,
    private val operation: OperationShape,
) : OperationCustomization() {
    private val sensitiveIndex = SensitiveIndex.of(codegenContext.model)
    override fun section(section: OperationSection): Writable = writable {
        if (section is OperationSection.AdditionalRuntimePluginConfig && sensitiveIndex.hasSensitiveOutput(operation)) {
            rustTemplate(
                """
                ${section.newLayerName}.store_put(#{SensitiveOutput});
                """,
                "SensitiveOutput" to RuntimeType.smithyRuntimeApi(codegenContext.runtimeConfig)
                    .resolve("client::orchestrator::SensitiveOutput"),
            )
        }
    }
}
