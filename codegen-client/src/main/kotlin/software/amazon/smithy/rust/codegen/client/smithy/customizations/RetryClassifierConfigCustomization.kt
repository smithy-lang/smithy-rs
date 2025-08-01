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
    private val moduleUseName = codegenContext.moduleUseName()

    private val retries = RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("client::retries")
    private val classifiers = retries.resolve("classifiers")
    private val codegenScope =
        arrayOf(
            "ClassifyRetry" to classifiers.resolve("ClassifyRetry"),
            "RetryStrategy" to retries.resolve("RetryStrategy"),
            "SharedRetryClassifier" to classifiers.resolve("SharedRetryClassifier"),
            "RetryClassifierPriority" to classifiers.resolve("RetryClassifierPriority"),
        )

    override fun section(section: ServiceConfig) =
        writable {
            when (section) {
                ServiceConfig.ConfigImpl ->
                    rustTemplate(
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
                        /// A retry classifier configured by this method will run according to its [priority](#{RetryClassifierPriority}).
                        ///
                        /// ## Examples
                        /// ```no_run
                        /// ## fn example() {
                        /// use aws_smithy_runtime_api::client::interceptors::context::InterceptorContext;
                        /// use aws_smithy_runtime_api::client::orchestrator::OrchestratorError;
                        /// use aws_smithy_runtime_api::client::retries::classifiers::{
                        ///     ClassifyRetry, RetryAction, RetryClassifierPriority,
                        /// };
                        /// use aws_smithy_types::error::metadata::ProvideErrorMetadata;
                        /// use aws_smithy_types::retry::ErrorKind;
                        /// use std::error::Error as StdError;
                        /// use std::marker::PhantomData;
                        /// use std::fmt;
                        /// use $moduleUseName::config::Config;
                        /// ## ##[derive(Debug)]
                        /// ## struct SomeOperationError {}
                        /// ## impl StdError for SomeOperationError {}
                        /// ## impl fmt::Display for SomeOperationError {
                        /// ##    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { todo!() }
                        /// ## }
                        /// ## impl ProvideErrorMetadata for SomeOperationError {
                        /// ##    fn meta(&self) -> &$moduleUseName::error::ErrorMetadata { todo!() }
                        /// ## }
                        ///
                        /// const RETRYABLE_ERROR_CODES: &[&str] = &[
                        ///     // List error codes to be retried here...
                        /// ];
                        ///
                        /// // When classifying at an operation's error type, classifiers require a generic parameter.
                        /// // When classifying the HTTP response alone, no generic is needed.
                        /// ##[derive(Debug, Default)]
                        /// pub struct ExampleErrorCodeClassifier<E> {
                        ///     _inner: PhantomData<E>,
                        /// }
                        ///
                        /// impl<E> ExampleErrorCodeClassifier<E> {
                        ///     pub fn new() -> Self {
                        ///         Self {
                        ///             _inner: PhantomData,
                        ///         }
                        ///     }
                        /// }
                        ///
                        /// impl<E> ClassifyRetry for ExampleErrorCodeClassifier<E>
                        /// where
                        ///     // Adding a trait bound for ProvideErrorMetadata allows us to inspect the error code.
                        ///     E: StdError + ProvideErrorMetadata + Send + Sync + 'static,
                        /// {
                        ///     fn classify_retry(&self, ctx: &InterceptorContext) -> RetryAction {
                        ///         // Check for a result
                        ///         let output_or_error = ctx.output_or_error();
                        ///         // Check for an error
                        ///         let error = match output_or_error {
                        ///             Some(Ok(_)) | None => return RetryAction::NoActionIndicated,
                        ///               Some(Err(err)) => err,
                        ///         };
                        ///
                        ///         // Downcast the generic error and extract the code
                        ///         let error_code = OrchestratorError::as_operation_error(error)
                        ///             .and_then(|err| err.downcast_ref::<E>())
                        ///             .and_then(|err| err.code());
                        ///
                        ///         // If this error's code is in our list, return an action that tells the RetryStrategy to retry this request.
                        ///         if let Some(error_code) = error_code {
                        ///             if RETRYABLE_ERROR_CODES.contains(&error_code) {
                        ///                 return RetryAction::transient_error();
                        ///             }
                        ///         }
                        ///
                        ///         // Otherwise, return that no action is indicated i.e. that this classifier doesn't require a retry.
                        ///         // Another classifier may still classify this response as retryable.
                        ///         RetryAction::NoActionIndicated
                        ///     }
                        ///
                        ///     fn name(&self) -> &'static str { "Example Error Code Classifier" }
                        /// }
                        ///
                        /// let config = Config::builder()
                        ///     .retry_classifier(ExampleErrorCodeClassifier::<SomeOperationError>::new())
                        ///     .build();
                        /// ## }
                        /// ```
                        pub fn retry_classifier(mut self, retry_classifier: impl #{ClassifyRetry} + 'static) -> Self {
                            self.push_retry_classifier(#{SharedRetryClassifier}::new(retry_classifier));
                            self
                        }

                        /// Like [`Self::retry_classifier`], but takes a [`SharedRetryClassifier`](#{SharedRetryClassifier}).
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

    override fun section(section: ServiceRuntimePluginSection): Writable =
        writable {
            when (section) {
                is ServiceRuntimePluginSection.RegisterRuntimeComponents -> {
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

    override fun section(section: OperationSection): Writable =
        writable {
            val classifiers = RuntimeType.smithyRuntime(runtimeConfig).resolve("client::retries::classifiers")

            val codegenScope =
                arrayOf(
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
