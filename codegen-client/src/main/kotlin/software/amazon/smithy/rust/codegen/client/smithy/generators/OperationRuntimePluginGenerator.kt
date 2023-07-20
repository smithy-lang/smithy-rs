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
import software.amazon.smithy.rust.codegen.client.smithy.customize.AuthSchemeOption
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
            "AuthSchemeOptionResolverParams" to runtimeApi.resolve("client::auth::AuthSchemeOptionResolverParams"),
            "BoxError" to RuntimeType.boxError(codegenContext.runtimeConfig),
            "ConfigBag" to RuntimeType.configBag(codegenContext.runtimeConfig),
            "Cow" to RuntimeType.Cow,
            "FrozenLayer" to smithyTypes.resolve("config_bag::FrozenLayer"),
            "Layer" to smithyTypes.resolve("config_bag::Layer"),
            "RetryClassifiers" to runtimeApi.resolve("client::retries::RetryClassifiers"),
            "RuntimeComponentsBuilder" to RuntimeType.runtimeComponentsBuilder(codegenContext.runtimeConfig),
            "RuntimePlugin" to RuntimeType.runtimePlugin(codegenContext.runtimeConfig),
            "SharedAuthSchemeOptionResolver" to runtimeApi.resolve("client::auth::SharedAuthSchemeOptionResolver"),
            "SharedRequestSerializer" to runtimeApi.resolve("client::ser_de::SharedRequestSerializer"),
            "SharedResponseDeserializer" to runtimeApi.resolve("client::ser_de::SharedResponseDeserializer"),
            "StaticAuthSchemeOptionResolver" to runtimeApi.resolve("client::auth::static_resolver::StaticAuthSchemeOptionResolver"),
            "StaticAuthSchemeOptionResolverParams" to runtimeApi.resolve("client::auth::static_resolver::StaticAuthSchemeOptionResolverParams"),
        )
    }

    fun render(
        writer: RustWriter,
        operationShape: OperationShape,
        operationStructName: String,
        authSchemeOptions: List<AuthSchemeOption>,
        customizations: List<OperationCustomization>,
    ) {
        writer.rustTemplate(
            """
            impl #{RuntimePlugin} for $operationStructName {
                fn config(&self) -> #{Option}<#{FrozenLayer}> {
                    let mut cfg = #{Layer}::new(${operationShape.id.name.dq()});

                    cfg.store_put(#{SharedRequestSerializer}::new(${operationStructName}RequestSerializer));
                    cfg.store_put(#{SharedResponseDeserializer}::new(${operationStructName}ResponseDeserializer));

                    ${"" /* TODO(IdentityAndAuth): Resolve auth parameters from input for services that need this */}
                    cfg.store_put(#{AuthSchemeOptionResolverParams}::new(#{StaticAuthSchemeOptionResolverParams}::new()));

                    #{additional_config}

                    #{Some}(cfg.freeze())
                }

                fn runtime_components(&self) -> #{Cow}<'_, #{RuntimeComponentsBuilder}> {
                    // Retry classifiers are operation-specific because they need to downcast operation-specific error types.
                    let retry_classifiers = #{RetryClassifiers}::new()
                        #{retry_classifier_customizations};

                    #{Cow}::Owned(
                        #{RuntimeComponentsBuilder}::new(${operationShape.id.name.dq()})
                            .with_retry_classifiers(#{Some}(retry_classifiers))
                            #{auth_options}
                            #{interceptors}
                    )
                }
            }

            #{runtime_plugin_supporting_types}
            """,
            *codegenScope,
            *preludeScope,
            "auth_options" to generateAuthOptions(operationShape, authSchemeOptions),
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
        authSchemeOptions: List<AuthSchemeOption>,
    ): Writable = writable {
        if (authSchemeOptions.any { it is AuthSchemeOption.CustomResolver }) {
            throw IllegalStateException("AuthSchemeOption.CustomResolver is unimplemented")
        } else {
            val authOptionsMap = authSchemeOptions.associate {
                val option = it as AuthSchemeOption.StaticAuthSchemeOption
                option.schemeShapeId to option
            }
            withBlockTemplate(
                """
                .with_auth_scheme_option_resolver(#{Some}(
                    #{SharedAuthSchemeOptionResolver}::new(
                        #{StaticAuthSchemeOptionResolver}::new(vec![
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
