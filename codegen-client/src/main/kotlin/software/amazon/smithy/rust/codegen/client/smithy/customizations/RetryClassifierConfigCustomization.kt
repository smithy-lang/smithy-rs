/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

class RetryClassifierConfigCustomization(codegenContext: ClientCodegenContext) : ConfigCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig

    private val classifiers = RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("client::retries::classifiers")
    private val codegenScope = arrayOf(
        "ClassifyRetry" to classifiers.resolve("ClassifyRetry"),
        "SharedRetryClassifier" to classifiers.resolve("SharedRetryClassifier"),
    )

    override fun section(section: ServiceConfig) =
        writable {
            when (section) {
                ServiceConfig.ConfigImpl -> rustTemplate(
                    """
                    /// Returns retry classifiers currently registered by the user.
                    pub fn retry_classifiers(&self) -> impl Iterator<Item = #{SharedRetryClassifier}> + '_ {
                        self.runtime_components.retry_classifiers()
                    }
                    """,
                    *codegenScope,
                )

                ServiceConfig.BuilderImpl ->
                    rustTemplate(
                        """
                        /// Add a retry classifier to the default chain of retry classifiers.
                        pub fn retry_classifier(mut self, retry_classifier: impl #{ClassifyRetry} + 'static) -> Self {
                            self.push_retry_classifier(#{SharedRetryClassifier}::new(retry_classifier));
                            self
                        }

                        /// Add a [`SharedRetryClassifier`] to the default chain of retry classifiers.
                        pub fn push_retry_classifier(&mut self, retry_classifier: #{SharedRetryClassifier}) -> &mut Self {
                            self.runtime_components.push_retry_classifier(retry_classifier);
                            self
                        }
                        """,
                        *codegenScope,
                    )

                else -> emptySection
            }
        }
}

class RetryClassifierServiceRuntimePluginCustomization(codegenContext: ClientCodegenContext) : ServiceRuntimePluginCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val retries = RuntimeType.smithyRuntime(runtimeConfig).resolve("client::retries")

    override fun section(section: ServiceRuntimePluginSection): Writable = writable {
        when (section) {
            is ServiceRuntimePluginSection.RegisterRuntimeComponents -> writable {
                rustTemplate(
                    "runtime_components.push_retry_classifier(#{HttpStatusCodeClassifier}::default().into());",
                    *RuntimeType.preludeScope,
                    "HttpStatusCodeClassifier" to retries.resolve("classifiers::HttpStatusCodeClassifier"),
                )
            }

            else -> emptySection
        }
    }
}

class RetryClassifierOperationCustomization(
    codegenContext: ClientCodegenContext,
    val operation: OperationShape,
) : OperationCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val symbolProvider = codegenContext.symbolProvider

    override fun section(section: OperationSection): Writable = writable {
        val classifiers = RuntimeType.smithyRuntime(runtimeConfig).resolve("client::retries::classifiers")

        val codegenScope = arrayOf(
            *RuntimeType.preludeScope,
            "SmithyErrorClassifier" to classifiers.resolve("SmithyErrorClassifier"),
            "ModeledAsRetryableClassifier" to classifiers.resolve("ModeledAsRetryableClassifier"),
            "OperationError" to symbolProvider.symbolForOperationError(operation),
        )

        when (section) {
            is OperationSection.RetryClassifiers -> {
                section.withRetryClassifier(this) {
                    rustTemplate(
                        "#{SmithyErrorClassifier}::<#{OperationError}>::new().into()",
                        *codegenScope,
                    )
                }
                section.withRetryClassifier(this) {
                    rustTemplate(
                        "#{ModeledAsRetryableClassifier}::<#{OperationError}>::new().into()",
                        *codegenScope,
                    )
                }
            }
            else -> emptySection
        }
    }
}
