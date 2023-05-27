/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations

/**
 * Generates operation-level runtime plugins
 */
class OperationRuntimePluginGenerator(
    codegenContext: ClientCodegenContext,
) {
    private val codegenScope = codegenContext.runtimeConfig.let { rc ->
        val runtimeApi = RuntimeType.smithyRuntimeApi(rc)
        val smithyTypes = RuntimeType.smithyTypes(rc)
        arrayOf(
            "AuthOptionResolverParams" to runtimeApi.resolve("client::auth::AuthOptionResolverParams"),
            "BoxError" to runtimeApi.resolve("client::runtime_plugin::BoxError"),
            "ConfigBag" to smithyTypes.resolve("config_bag::ConfigBag"),
            "ConfigBagAccessors" to runtimeApi.resolve("client::orchestrator::ConfigBagAccessors"),
            "InterceptorRegistrar" to runtimeApi.resolve("client::interceptors::InterceptorRegistrar"),
            "RetryClassifiers" to runtimeApi.resolve("client::retries::RetryClassifiers"),
            "RuntimePlugin" to runtimeApi.resolve("client::runtime_plugin::RuntimePlugin"),
            "StaticAuthOptionResolverParams" to runtimeApi.resolve("client::auth::option_resolver::StaticAuthOptionResolverParams"),
        )
    }

    fun render(
        writer: RustWriter,
        operationShape: OperationShape,
        operationStructName: String,
        customizations: List<OperationCustomization>,
    ) {
        writer.rustTemplate(
            """
            impl #{RuntimePlugin} for $operationStructName {
                fn configure(&self, cfg: &mut #{ConfigBag}, _interceptors: &mut #{InterceptorRegistrar}) -> Result<(), #{BoxError}> {
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
                    OperationSection.AdditionalRuntimePluginConfig(customizations, "cfg", "_interceptors", operationShape),
                )
            },
            "retry_classifier_customizations" to writable {
                writeCustomizations(customizations, OperationSection.RetryClassifier(customizations, "cfg", operationShape))
            },
            "runtime_plugin_supporting_types" to writable {
                writeCustomizations(
                    customizations,
                    OperationSection.RuntimePluginSupportingTypes(customizations, "cfg", operationShape),
                )
            },
        )
    }
}
