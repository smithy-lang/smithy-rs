/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.util.letIf

/**
 * Decorator that injects operation-level interceptors that configure an endpoint parameters builder
 * with operation specific information, e.g. a bucket name.
 *
 * Whenever a setter needs to be called on the endpoint parameters builder with operation specific information,
 * this decorator must be used.
 */
class EndpointParamsDecorator : ClientCodegenDecorator {
    override val name: String get() = "EndpointParamsDecorator"
    override val order: Byte get() = 0

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> =
        baseCustomizations.letIf(codegenContext.smithyRuntimeMode.generateOrchestrator) {
            it + listOf(EndpointParametersCustomization(codegenContext, operation))
        }
}

private class EndpointParametersCustomization(
    private val codegenContext: ClientCodegenContext,
    private val operation: OperationShape,
) : OperationCustomization() {
    override fun section(section: OperationSection): Writable = writable {
        val symbolProvider = codegenContext.symbolProvider
        val operationName = symbolProvider.toSymbol(operation).name
        if (section is OperationSection.AdditionalInterceptors) {
            section.registerInterceptor(codegenContext.runtimeConfig, this) {
                rust("${operationName}EndpointParamsInterceptor")
            }
        }
    }
}
