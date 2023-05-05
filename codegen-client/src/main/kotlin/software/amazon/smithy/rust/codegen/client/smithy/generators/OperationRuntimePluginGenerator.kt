/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations

sealed class OperationRuntimePluginSection(name: String) : Section(name) {
    /**
     * Hook for adding additional things to config inside operation runtime plugins.
     */
    data class AdditionalConfig(
        val configBagName: String,
        val interceptorName: String,
        val operationShape: OperationShape,
    ) : OperationRuntimePluginSection("AdditionalConfig") {
        fun registerInterceptor(runtimeConfig: RuntimeConfig, writer: RustWriter, interceptor: Writable) {
            val smithyRuntimeApi = RuntimeType.smithyRuntimeApi(runtimeConfig)
            writer.rustTemplate(
                """
                $interceptorName.register_operation_interceptor(std::sync::Arc::new(#{interceptor}) as _);
                """,
                "HttpRequest" to smithyRuntimeApi.resolve("client::orchestrator::HttpRequest"),
                "HttpResponse" to smithyRuntimeApi.resolve("client::orchestrator::HttpResponse"),
                "Interceptors" to smithyRuntimeApi.resolve("client::interceptors::Interceptors"),
                "interceptor" to interceptor,
            )
        }
    }

    /**
     * Hook for adding retry classifiers to an operation's `RetryClassifiers` bundle.
     *
     * Should emit 1+ lines of code that look like the following:
     * ```rust
     * .with_classifier(AwsErrorCodeClassifier::new())
     * .with_classifier(HttpStatusCodeClassifier::new())
     * ```
     */
    data class RetryClassifier(
        val configBagName: String,
        val operationShape: OperationShape,
    ) : OperationRuntimePluginSection("RetryClassifier")

    /**
     * Hook for adding supporting types for operation-specific runtime plugins.
     * Examples include various operation-specific types (retry classifiers, config bag types, etc.)
     */
    data class RuntimePluginSupportingTypes(
        val configBagName: String,
        val operationShape: OperationShape,
    ) : OperationRuntimePluginSection("RuntimePluginSupportingTypes")
}

typealias OperationRuntimePluginCustomization = NamedCustomization<OperationRuntimePluginSection>

/**
 * Generates operation-level runtime plugins
 */
class OperationRuntimePluginGenerator(
    codegenContext: ClientCodegenContext,
) {
    private val codegenScope = codegenContext.runtimeConfig.let { rc ->
        val runtimeApi = RuntimeType.smithyRuntimeApi(rc)
        arrayOf(
            "StaticAuthOptionResolverParams" to runtimeApi.resolve("client::auth::option_resolver::StaticAuthOptionResolverParams"),
            "AuthOptionResolverParams" to runtimeApi.resolve("client::auth::AuthOptionResolverParams"),
            "BoxError" to runtimeApi.resolve("client::runtime_plugin::BoxError"),
            "ConfigBag" to runtimeApi.resolve("config_bag::ConfigBag"),
            "ConfigBagAccessors" to runtimeApi.resolve("client::orchestrator::ConfigBagAccessors"),
            "RetryClassifiers" to runtimeApi.resolve("client::retries::RetryClassifiers"),
            "RuntimePlugin" to runtimeApi.resolve("client::runtime_plugin::RuntimePlugin"),
            "Interceptors" to runtimeApi.resolve("client::interceptors::Interceptors"),
        )
    }

    fun render(
        writer: RustWriter,
        operationShape: OperationShape,
        operationStructName: String,
        customizations: List<OperationRuntimePluginCustomization>,
    ) {
        writer.rustTemplate(
            """
            impl #{RuntimePlugin} for $operationStructName {
                fn configure(&self, cfg: &mut #{ConfigBag}, _interceptors: &mut #{Interceptors}) -> Result<(), #{BoxError}> {
                    use #{ConfigBagAccessors} as _;
                    cfg.set_request_serializer(${operationStructName}RequestSerializer);
                    cfg.set_response_deserializer(${operationStructName}ResponseDeserializer);

                    ${"" /* TODO(IdentityAndAuth): Resolve auth parameters from input for services that need this */}
                    cfg.set_auth_option_resolver_params(#{AuthOptionResolverParams}::new(#{StaticAuthOptionResolverParams}::new()));

                    // Retry classifiers are operation-specific because they need to downcast operation-specific error types.
                    let retry_classifiers = #{RetryClassifiers}::new()
                        #{retry_classifier_customizations};
                    cfg.set_retry_classifiers(retry_classifiers);

                    #{additional_config}
                    Ok(())
                }
            }

            #{runtime_plugin_supporting_types}
            """,
            *codegenScope,
            "additional_config" to writable {
                writeCustomizations(
                    customizations,
                    OperationRuntimePluginSection.AdditionalConfig("cfg", "_interceptors", operationShape),
                )
            },
            "retry_classifier_customizations" to writable {
                writeCustomizations(customizations, OperationRuntimePluginSection.RetryClassifier("cfg", operationShape))
            },
            "runtime_plugin_supporting_types" to writable {
                writeCustomizations(
                    customizations,
                    OperationRuntimePluginSection.RuntimePluginSupportingTypes("cfg", operationShape),
                )
            },
        )
    }
}
