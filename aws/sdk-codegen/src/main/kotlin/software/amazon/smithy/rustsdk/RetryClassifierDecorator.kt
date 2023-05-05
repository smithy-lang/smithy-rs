/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationRuntimePluginSection
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.derive
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection

class RetryClassifierDecorator : ClientCodegenDecorator {
    override val name: String = "RetryPolicy"
    override val order: Byte = 0

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> =
        baseCustomizations + RetryClassifierFeature(codegenContext.runtimeConfig)

    override fun operationRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationRuntimePluginCustomization>,
    ): List<OperationRuntimePluginCustomization> =
        baseCustomizations + OperationRetryClassifiersFeature(codegenContext, operation)
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
) : OperationRuntimePluginCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val awsRuntime = AwsRuntimeType.awsRuntime(runtimeConfig)
    private val smithyRuntime = RuntimeType.smithyRuntime(runtimeConfig)
    private val smithyRuntimeApi = RuntimeType.smithyRuntimeApi(runtimeConfig)
    private val codegenScope = arrayOf(
        "HttpStatusCodeClassifier" to smithyRuntime.resolve("client::retries::classifier::HttpStatusCodeClassifier"),
        "AwsErrorCodeClassifier" to awsRuntime.resolve("retries::classifier::AwsErrorCodeClassifier"),
        "ModeledAsRetryableClassifier" to smithyRuntime.resolve("client::retries::classifier::ModeledAsRetryableClassifier"),
        "AmzRetryAfterHeaderClassifier" to awsRuntime.resolve("retries::classifier::AmzRetryAfterHeaderClassifier"),
        "SmithyErrorClassifier" to smithyRuntime.resolve("client::retries::classifier::SmithyErrorClassifier"),
        "RetryReason" to smithyRuntimeApi.resolve("client::retries::RetryReason"),
        "ClassifyRetry" to smithyRuntimeApi.resolve("client::retries::ClassifyRetry"),
        "RetryClassifiers" to smithyRuntimeApi.resolve("client::retries::RetryClassifiers"),
        "OperationError" to codegenContext.symbolProvider.symbolForOperationError(operation),
        "SdkError" to RuntimeType.smithyHttp(runtimeConfig).resolve("result::SdkError"),
        "ErasedError" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("type_erasure::TypeErasedError"),
    )

    override fun section(section: OperationRuntimePluginSection) = when (section) {
        is OperationRuntimePluginSection.RuntimePluginSupportingTypes -> writable {
            Attribute(derive(RuntimeType.Debug)).render(this)
            rustTemplate(
                """
                struct HttpStatusCodeClassifier(#{HttpStatusCodeClassifier});
                impl HttpStatusCodeClassifier {
                    fn new() -> Self {
                        Self(#{HttpStatusCodeClassifier}::default())
                    }
                }
                impl #{ClassifyRetry} for HttpStatusCodeClassifier {
                    fn classify_retry(&self, error: &#{ErasedError}) -> Option<#{RetryReason}> {
                        let error = error.downcast_ref::<#{SdkError}<#{OperationError}>>().expect("The error type is always known");
                        self.0.classify_error(error)
                    }
                }
                """,
                *codegenScope,
            )

            Attribute(derive(RuntimeType.Debug)).render(this)
            rustTemplate(
                """
                struct AwsErrorCodeClassifier(#{AwsErrorCodeClassifier});
                impl AwsErrorCodeClassifier {
                    fn new() -> Self {
                        Self(#{AwsErrorCodeClassifier})
                    }
                }
                impl #{ClassifyRetry} for AwsErrorCodeClassifier {
                    fn classify_retry(&self, error: &#{ErasedError}) -> Option<#{RetryReason}> {
                        let error = error.downcast_ref::<#{SdkError}<#{OperationError}>>().expect("The error type is always known");
                        self.0.classify_error(error)
                    }
                }
                """,
                *codegenScope,
            )

            Attribute(derive(RuntimeType.Debug)).render(this)
            rustTemplate(
                """
                struct ModeledAsRetryableClassifier(#{ModeledAsRetryableClassifier});
                impl ModeledAsRetryableClassifier {
                    fn new() -> Self {
                        Self(#{ModeledAsRetryableClassifier})
                    }
                }
                impl #{ClassifyRetry} for ModeledAsRetryableClassifier {
                    fn classify_retry(&self, error: &#{ErasedError}) -> Option<#{RetryReason}> {
                        let error = error.downcast_ref::<#{SdkError}<#{OperationError}>>().expect("The error type is always known");
                        self.0.classify_error(error)
                    }
                }
                """,
                *codegenScope,
            )

            Attribute(derive(RuntimeType.Debug)).render(this)
            rustTemplate(
                """
                struct AmzRetryAfterHeaderClassifier(#{AmzRetryAfterHeaderClassifier});
                impl AmzRetryAfterHeaderClassifier {
                    fn new() -> Self {
                        Self(#{AmzRetryAfterHeaderClassifier})
                    }
                }
                impl #{ClassifyRetry} for AmzRetryAfterHeaderClassifier {
                    fn classify_retry(&self, error: &#{ErasedError}) -> Option<#{RetryReason}> {
                        let error = error.downcast_ref::<#{SdkError}<#{OperationError}>>().expect("The error type is always known");
                        self.0.classify_error(error)
                    }
                }
                """,
                *codegenScope,
            )

            Attribute(derive(RuntimeType.Debug)).render(this)
            rustTemplate(
                """
                struct SmithyErrorClassifier(#{SmithyErrorClassifier});
                impl SmithyErrorClassifier {
                    fn new() -> Self {
                        Self(#{SmithyErrorClassifier})
                    }
                }
                impl #{ClassifyRetry} for SmithyErrorClassifier {
                    fn classify_retry(&self, error: &#{ErasedError}) -> Option<#{RetryReason}> {
                        let error = error.downcast_ref::<#{SdkError}<#{OperationError}>>().expect("The error type is always known");
                        self.0.classify_error(error)
                    }
                }
                """,
                *codegenScope,
            )
        }

        is OperationRuntimePluginSection.RetryClassifier -> writable {
            rustTemplate(
                """
                .with_classifier(SmithyErrorClassifier::new())
                .with_classifier(AmzRetryAfterHeaderClassifier::new())
                .with_classifier(ModeledAsRetryableClassifier::new())
                .with_classifier(AwsErrorCodeClassifier::new())
                .with_classifier(HttpStatusCodeClassifier::new())
                """,
                *codegenScope,
            )
        }

        else -> emptySection
    }
}
