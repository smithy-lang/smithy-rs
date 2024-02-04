/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable

class RetryClassifierDecorator : ClientCodegenDecorator {
    override val name: String = "RetryPolicy"
    override val order: Byte = 0

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> =
        baseCustomizations +
            OperationRetryClassifiersFeature(codegenContext, operation)
}

class OperationRetryClassifiersFeature(
    codegenContext: ClientCodegenContext,
    val operation: OperationShape,
) : OperationCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val symbolProvider = codegenContext.symbolProvider

    override fun section(section: OperationSection) =
        when (section) {
            is OperationSection.RetryClassifiers ->
                writable {
                    section.registerRetryClassifier(this) {
                        rustTemplate(
                            "#{AwsErrorCodeClassifier}::<#{OperationError}>::new()",
                            "AwsErrorCodeClassifier" to AwsRuntimeType.awsRuntime(runtimeConfig).resolve("retries::classifiers::AwsErrorCodeClassifier"),
                            "OperationError" to symbolProvider.symbolForOperationError(operation),
                        )
                    }
                }

            else -> emptySection
        }
}
