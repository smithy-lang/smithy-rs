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

/**
 * Registers the clock-skew retry classifier on every operation.
 *
 * This is applied globally, including to S3 and STS. Those services opt out of
 * [RetryClassifierDecorator] so they can customize `AwsErrorCodeClassifier`, but the clock-skew
 * classifier is a separate, orthogonal classifier that every service needs, so it is registered
 * here rather than in that decorator.
 */
class ClockSkewRetryClassifierDecorator : ClientCodegenDecorator {
    override val name: String = "ClockSkewRetryClassifier"
    override val order: Byte = 0

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> = baseCustomizations + ClockSkewRetryClassifierFeature(codegenContext, operation)
}

private class ClockSkewRetryClassifierFeature(
    codegenContext: ClientCodegenContext,
    private val operation: OperationShape,
) : OperationCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val symbolProvider = codegenContext.symbolProvider

    override fun section(section: OperationSection) =
        when (section) {
            is OperationSection.RetryClassifiers ->
                writable {
                    section.registerRetryClassifier(this) {
                        rustTemplate(
                            "#{ServiceClockSkewClassifier}::<#{OperationError}>::new()",
                            "ServiceClockSkewClassifier" to
                                AwsRuntimeType.awsRuntime(runtimeConfig)
                                    .resolve("service_clock_skew::ServiceClockSkewClassifier"),
                            "OperationError" to symbolProvider.symbolForOperationError(operation),
                        )
                    }
                }

            else -> emptySection
        }
}
