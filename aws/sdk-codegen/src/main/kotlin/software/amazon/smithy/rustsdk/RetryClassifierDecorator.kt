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
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.letIf

class RetryClassifierDecorator : ClientCodegenDecorator {
    override val name: String = "RetryPolicy"
    override val order: Byte = 0

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> =
        (baseCustomizations + RetryClassifierFeature(codegenContext.runtimeConfig)).letIf(codegenContext.smithyRuntimeMode.generateOrchestrator) {
            it + OperationRetryClassifiersFeature(
                codegenContext,
                operation,
            )
        }
}

class RetryClassifierFeature(private val runtimeConfig: RuntimeConfig) : OperationCustomization() {
    override fun retryType(): RuntimeType =
        AwsRuntimeType.awsHttp(runtimeConfig).resolve("retry::AwsResponseRetryClassifier")

    override fun section(section: OperationSection) = when (section) {
        is OperationSection.FinalizeOperation -> writable {
            rust(
                "let ${section.operation} = ${section.operation}.with_retry_classifier(#T::new());",
                retryType(),
            )
        }

        else -> emptySection
    }
}

class OperationRetryClassifiersFeature(
    codegenContext: ClientCodegenContext,
    operation: OperationShape,
) : OperationCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val awsRuntime = AwsRuntimeType.awsRuntime(runtimeConfig)
    private val smithyRuntime = RuntimeType.smithyRuntime(runtimeConfig)
    private val smithyRuntimeApi = RuntimeType.smithyRuntimeApi(runtimeConfig)
    private val codegenScope = arrayOf(
        // Classifiers
        "SmithyErrorClassifier" to smithyRuntime.resolve("client::retries::classifier::SmithyErrorClassifier"),
        "AmzRetryAfterHeaderClassifier" to awsRuntime.resolve("retries::classifier::AmzRetryAfterHeaderClassifier"),
        "ModeledAsRetryableClassifier" to smithyRuntime.resolve("client::retries::classifier::ModeledAsRetryableClassifier"),
        "AwsErrorCodeClassifier" to awsRuntime.resolve("retries::classifier::AwsErrorCodeClassifier"),
        "HttpStatusCodeClassifier" to smithyRuntime.resolve("client::retries::classifier::HttpStatusCodeClassifier"),
        // Other Types
        "ClassifyRetry" to smithyRuntimeApi.resolve("client::retries::ClassifyRetry"),
        "InterceptorContext" to RuntimeType.interceptorContext(runtimeConfig),
        "OperationError" to codegenContext.symbolProvider.symbolForOperationError(operation),
        "OrchestratorError" to smithyRuntimeApi.resolve("client::orchestrator::OrchestratorError"),
        "RetryReason" to smithyRuntimeApi.resolve("client::retries::RetryReason"),
    )

    override fun section(section: OperationSection) = when (section) {
        is OperationSection.RetryClassifier -> writable {
            rustTemplate(
                """
                .with_classifier(#{SmithyErrorClassifier}::<#{OperationError}>::new())
                .with_classifier(#{AmzRetryAfterHeaderClassifier})
                .with_classifier(#{ModeledAsRetryableClassifier}::<#{OperationError}>::new())
                .with_classifier(#{AwsErrorCodeClassifier}::<#{OperationError}>::new())
                .with_classifier(#{HttpStatusCodeClassifier}::default())
                """,
                *codegenScope,
            )
        }

        else -> emptySection
    }
}
