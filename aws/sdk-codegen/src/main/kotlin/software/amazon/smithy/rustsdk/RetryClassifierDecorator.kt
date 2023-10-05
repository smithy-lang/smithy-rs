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
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable

class RetryClassifierDecorator : ClientCodegenDecorator {
    override val name: String = "RetryPolicy"
    override val order: Byte = 0

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> = baseCustomizations +
        OperationRetryClassifiersFeature(codegenContext, operation)

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> = baseCustomizations +
        ServiceRetryClassifiersFeature(codegenContext)
}

class OperationRetryClassifiersFeature(
    codegenContext: ClientCodegenContext,
    val operation: OperationShape,
) : OperationCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val symbolProvider = codegenContext.symbolProvider

    override fun section(section: OperationSection) = when (section) {
        is OperationSection.RetryClassifiers -> writable {
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

class ServiceRetryClassifiersFeature(
    codegenContext: ClientCodegenContext,
) : ServiceRuntimePluginCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig

    override fun section(section: ServiceRuntimePluginSection) = when (section) {
        is ServiceRuntimePluginSection.RegisterRuntimeComponents -> writable {
            section.registerRetryClassifier(this) {
                rustTemplate(
                    "#{AmzRetryAfterHeaderClassifier}::default()",
                    "AmzRetryAfterHeaderClassifier" to AwsRuntimeType.awsRuntime(runtimeConfig).resolve("retries::classifiers::AmzRetryAfterHeaderClassifier"),
                )
            }
        }

        else -> emptySection
    }
}
