/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.shapes.OperationShape
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
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import java.util.logging.Logger

/**
 * Generates operation-level runtime plugins
 */
class OperationRuntimePluginGenerator(
    private val codegenContext: ClientCodegenContext,
) {
    private val logger: Logger = Logger.getLogger(javaClass.name)
    private val codegenScope = codegenContext.runtimeConfig.let { rc ->
        val runtimeApi = RuntimeType.smithyRuntimeApi(rc)
        val smithyTypes = RuntimeType.smithyTypes(rc)
        arrayOf(
            *preludeScope,
            "AuthOptionResolverParams" to runtimeApi.resolve("client::auth::AuthOptionResolverParams"),
            "BoxError" to RuntimeType.boxError(codegenContext.runtimeConfig),
            "ConfigBag" to RuntimeType.configBag(codegenContext.runtimeConfig),
            "ConfigBagAccessors" to RuntimeType.configBagAccessors(codegenContext.runtimeConfig),
            "Cow" to RuntimeType.Cow,
            "SharedAuthOptionResolver" to runtimeApi.resolve("client::auth::SharedAuthOptionResolver"),
            "DynResponseDeserializer" to runtimeApi.resolve("client::orchestrator::DynResponseDeserializer"),
            "FrozenLayer" to smithyTypes.resolve("config_bag::FrozenLayer"),
            "Layer" to smithyTypes.resolve("config_bag::Layer"),
            "RetryClassifiers" to runtimeApi.resolve("client::retries::RetryClassifiers"),
            "RuntimePlugin" to RuntimeType.runtimePlugin(codegenContext.runtimeConfig),
            "RuntimeComponentsBuilder" to RuntimeType.runtimeComponentsBuilder(codegenContext.runtimeConfig),
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

                    ${"" /* TODO(IdentityAndAuth): Resolve auth parameters from input for services that need this */}
                    cfg.set_auth_option_resolver_params(#{AuthOptionResolverParams}::new(#{StaticAuthOptionResolverParams}::new()));

                    #{additional_config}

                    Some(cfg.freeze())
                }

                fn runtime_components(&self) -> #{Cow}<'_, #{RuntimeComponentsBuilder}> {
                    // Retry classifiers are operation-specific because they need to downcast operation-specific error types.
                    let retry_classifiers = #{RetryClassifiers}::new()
                        #{retry_classifier_customizations};

                    #{Cow}::Owned(
                        #{RuntimeComponentsBuilder}::new(${operationShape.id.name.dq()})
                            .with_retry_classifiers(Some(retry_classifiers))
                            #{auth_options}
                            #{interceptors}
                    )
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
                    OperationSection.AdditionalInterceptors(customizations, operationShape),
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
            withBlockTemplate(
                """
                .with_auth_option_resolver(#{Some}(
                    #{SharedAuthOptionResolver}::new(
                        #{StaticAuthOptionResolver}::new(vec![
                """,
                "]))))",
                *codegenScope,
            ) {
                var noSupportedAuthSchemes = true
                val authSchemes = ServiceIndex.of(codegenContext.model)
                    .getEffectiveAuthSchemes(codegenContext.serviceShape, operationShape)
                for (schemeShapeId in authSchemes.keys) {
                    val authOption = authOptionsMap[schemeShapeId]
                    if (authOption != null) {
                        authOption.constructor(this)
                        noSupportedAuthSchemes = false
                    } else {
                        logger.warning(
                            "No auth scheme implementation available for $schemeShapeId. " +
                                "The generated client will not attempt to use this auth scheme.",
                        )
                    }
                }
                if (operationShape.hasTrait<OptionalAuthTrait>() || noSupportedAuthSchemes) {
                    val authOption = authOptionsMap[noAuthSchemeShapeId]
                        ?: throw IllegalStateException("Missing 'no auth' implementation. This is a codegen bug.")
                    authOption.constructor(this)
                }
            }
        }
    }
}
