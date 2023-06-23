/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.AuthTrait
import software.amazon.smithy.model.traits.OptionalAuthTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customizations.noAuthSchemeShapeId
import software.amazon.smithy.rust.codegen.client.smithy.customize.AuthOption
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait

/**
 * Generates operation-level runtime plugins
 */
class OperationRuntimePluginGenerator(
    private val codegenContext: ClientCodegenContext,
) {
    private val codegenScope = codegenContext.runtimeConfig.let { rc ->
        val runtimeApi = RuntimeType.smithyRuntimeApi(rc)
        val smithyTypes = RuntimeType.smithyTypes(rc)
        arrayOf(
            "AuthOptionResolverParams" to runtimeApi.resolve("client::auth::AuthOptionResolverParams"),
            "BoxError" to RuntimeType.boxError(codegenContext.runtimeConfig),
            "ConfigBag" to RuntimeType.configBag(codegenContext.runtimeConfig),
            "ConfigBagAccessors" to RuntimeType.configBagAccessors(codegenContext.runtimeConfig),
            "DynAuthOptionResolver" to runtimeApi.resolve("client::auth::DynAuthOptionResolver"),
            "DynResponseDeserializer" to runtimeApi.resolve("client::orchestrator::DynResponseDeserializer"),
            "FrozenLayer" to smithyTypes.resolve("config_bag::FrozenLayer"),
            "InterceptorRegistrar" to runtimeApi.resolve("client::interceptors::InterceptorRegistrar"),
            "Layer" to smithyTypes.resolve("config_bag::Layer"),
            "RetryClassifiers" to runtimeApi.resolve("client::retries::RetryClassifiers"),
            "RuntimePlugin" to runtimeApi.resolve("client::runtime_plugin::RuntimePlugin"),
            "SharedRequestSerializer" to runtimeApi.resolve("client::orchestrator::SharedRequestSerializer"),
            "StaticAuthOptionResolver" to runtimeApi.resolve("client::auth::option_resolver::StaticAuthOptionResolver"),
            "StaticAuthOptionResolverParams" to runtimeApi.resolve("client::auth::option_resolver::StaticAuthOptionResolverParams"),
        )
    }

    fun render(
        writer: RustWriter,
        operationShape: OperationShape,
        operationStructName: String,
        authOptions: List<AuthOption>,
        customizations: List<OperationCustomization>,
    ) {
        writer.rustTemplate(
            """
            impl #{RuntimePlugin} for $operationStructName {
                fn config(&self) -> #{Option}<#{FrozenLayer}> {
                    let mut cfg = #{Layer}::new(${operationShape.id.name.dq()});
                    use #{ConfigBagAccessors} as _;
                    cfg.set_request_serializer(#{SharedRequestSerializer}::new(${operationStructName}RequestSerializer));
                    cfg.set_response_deserializer(#{DynResponseDeserializer}::new(${operationStructName}ResponseDeserializer));

                    // Retry classifiers are operation-specific because they need to downcast operation-specific error types.
                    let retry_classifiers = #{RetryClassifiers}::new()
                        #{retry_classifier_customizations};
                    cfg.set_retry_classifiers(retry_classifiers);

                    ${"" /* TODO(IdentityAndAuth): Resolve auth parameters from input for services that need this */}
                    cfg.set_auth_option_resolver_params(#{AuthOptionResolverParams}::new(#{StaticAuthOptionResolverParams}::new()));

                    #{auth_options}
                    #{additional_config}

                    Some(cfg.freeze())
                }

                fn interceptors(&self, _interceptors: &mut #{InterceptorRegistrar}) {
                    #{interceptors}
                }
            }

            #{runtime_plugin_supporting_types}
            """,
            *codegenScope,
            *preludeScope,
            "auth_options" to generateAuthOptions(operationShape, authOptions),
            "additional_config" to writable {
                writeCustomizations(
                    customizations,
                    OperationSection.AdditionalRuntimePluginConfig(
                        customizations,
                        newLayerName = "cfg",
                        operationShape,
                    ),
                )
            },
            "retry_classifier_customizations" to writable {
                writeCustomizations(
                    customizations,
                    OperationSection.RetryClassifier(customizations, "cfg", operationShape),
                )
            },
            "runtime_plugin_supporting_types" to writable {
                writeCustomizations(
                    customizations,
                    OperationSection.RuntimePluginSupportingTypes(customizations, "cfg", operationShape),
                )
            },
            "interceptors" to writable {
                writeCustomizations(
                    customizations,
                    OperationSection.AdditionalInterceptors(customizations, "_interceptors", operationShape),
                )
            },
        )
    }

    private fun generateAuthOptions(
        operationShape: OperationShape,
        authOptions: List<AuthOption>,
    ): Writable = writable {
        if (authOptions.any { it is AuthOption.CustomResolver }) {
            throw IllegalStateException("AuthOption.CustomResolver is unimplemented")
        } else {
            val authOptionsMap = authOptions.associate {
                val option = it as AuthOption.StaticAuthOption
                option.schemeShapeId to option
            }
            val authTrait: AuthTrait? = operationShape.getTrait() ?: codegenContext.serviceShape.getTrait()
            withBlockTemplate(
                "cfg.set_auth_option_resolver(#{DynAuthOptionResolver}::new(#{StaticAuthOptionResolver}::new(vec![",
                "])));",
                *codegenScope,
            ) {
                var atLeastOneScheme = false
                for (schemeShapeId in authTrait?.valueSet ?: emptyList()) {
                    val authOption = authOptionsMap[schemeShapeId]
                        ?: throw IllegalStateException("no auth scheme implementation available for $schemeShapeId")
                    authOption.constructor(this)
                    atLeastOneScheme = true
                }
                if (operationShape.hasTrait<OptionalAuthTrait>()) {
                    val authOption = authOptionsMap[noAuthSchemeShapeId]
                        ?: throw IllegalStateException("missing 'no auth' implementation")
                    authOption.constructor(this)
                    atLeastOneScheme = true
                }
                if (!atLeastOneScheme) {
                    throw IllegalStateException(
                        "this client won't have any auth schemes " +
                            "(not even optional/no-auth auth), which means the generated client " +
                            "won't work at all for the ${operationShape.id} operation",
                    )
                }
            }
        }
    }
}
