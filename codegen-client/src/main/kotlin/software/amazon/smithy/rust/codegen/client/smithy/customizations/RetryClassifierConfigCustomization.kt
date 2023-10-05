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
                        /// Add type implementing [`ClassifyRetry`](#{ClassifyRetry}) that will be used by the
                        /// [`RetryStrategy`](#{RetryStrategy}) to determine what responses should be retried.
                        ///
                        /// A retry classifier configured by this method will run according to its priority.
                        ///
                        /// ## Examples
                        /// ```no_run
                        /// ## ##[cfg(test)]
                        /// ## mod tests {
                        /// ## ##[test]
                        /// ## fn example() {
                        /// ## }
                        /// ## }
                        /// ```
                        pub fn retry_classifier(mut self, retry_classifier: impl #{ClassifyRetry} + 'static) -> Self {
                            self.push_retry_classifier(#{SharedRetryClassifier}::new(retry_classifier));
                            self
                        }

                        /// Add a [`SharedRetryClassifier`](#{SharedRetryClassifier}) that will be used by the
                        /// [`RetryStrategy`](#{RetryStrategy}) to determine what responses should be retried.
                        ///
                        /// A retry classifier configured by this method will run according to its priority.
                        ///
                        /// ## Examples
                        /// ```no_run
                        /// ## ##[cfg(test)]
                        /// ## mod tests {
                        /// ## ##[test]
                        /// ## fn example() {
                        /// ## }
                        /// ## }
                        /// ```
                        pub fn push_retry_classifier(&mut self, retry_classifier: #{SharedRetryClassifier}) -> &mut Self {
                            self.runtime_components.push_retry_classifier(retry_classifier);
                            self
                        }

                        /// Set [`SharedRetryClassifier`](#{SharedRetryClassifier})s for the builder, replacing any that
                        /// were previously set.
                        pub fn set_retry_classifiers(&mut self, retry_classifiers: impl IntoIterator<Item = #{SharedRetryClassifier}>) -> &mut Self {
                            self.runtime_components.set_retry_classifiers(retry_classifiers.into_iter());
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
                section.registerRetryClassifier(this) {
                    rustTemplate(
                        "#{HttpStatusCodeClassifier}::default()",
                        "HttpStatusCodeClassifier" to retries.resolve("classifiers::HttpStatusCodeClassifier"),
                    )
                }
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
            "TransientErrorClassifier" to classifiers.resolve("TransientErrorClassifier"),
            "ModeledAsRetryableClassifier" to classifiers.resolve("ModeledAsRetryableClassifier"),
            "OperationError" to symbolProvider.symbolForOperationError(operation),
        )

        when (section) {
            is OperationSection.RetryClassifiers -> {
                section.registerRetryClassifier(this) {
                    rustTemplate(
                        "#{TransientErrorClassifier}::<#{OperationError}>::new()",
                        *codegenScope,
                    )
                }
                section.registerRetryClassifier(this) {
                    rustTemplate(
                        "#{ModeledAsRetryableClassifier}::<#{OperationError}>::new()",
                        *codegenScope,
                    )
                }
            }
            else -> emptySection
        }
    }
}
