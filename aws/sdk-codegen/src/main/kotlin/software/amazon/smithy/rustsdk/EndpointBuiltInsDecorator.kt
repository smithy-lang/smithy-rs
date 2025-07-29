/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.BooleanNode
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rulesengine.aws.language.functions.AwsBuiltIns
import software.amazon.smithy.rulesengine.language.EndpointRuleSet
import software.amazon.smithy.rulesengine.language.syntax.parameters.BuiltIns
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameter
import software.amazon.smithy.rulesengine.language.syntax.parameters.ParameterType
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointCustomization
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointRulesetIndex
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rustName
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.symbol
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigParam
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.configParamNewtype
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.loadFromConfigBag
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.standardConfigParam
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.mapRustType
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.extendIf
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.sdkId
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import java.util.Optional

/** load a builtIn parameter from a ruleset by name */
fun EndpointRuleSet.getBuiltIn(builtIn: String) = parameters.toList().find { it.builtIn == Optional.of(builtIn) }

/** load a builtIn parameter from a ruleset. The returned builtIn is the one defined in the ruleset (including latest docs, etc.) */
fun EndpointRuleSet.getBuiltIn(builtIn: Parameter) = getBuiltIn(builtIn.builtIn.orNull()!!)

fun ClientCodegenContext.getBuiltIn(builtIn: Parameter): Parameter? = getBuiltIn(builtIn.builtIn.orNull()!!)

fun ClientCodegenContext.getBuiltIn(builtIn: String): Parameter? {
    val idx = EndpointRulesetIndex.of(model)
    val rules = idx.endpointRulesForService(serviceShape) ?: return null
    return rules.getBuiltIn(builtIn)
}

private fun promotedBuiltins(parameter: Parameter) =
    parameter.builtIn == AwsBuiltIns.FIPS.builtIn ||
        parameter.builtIn == AwsBuiltIns.DUALSTACK.builtIn ||
        parameter.builtIn == BuiltIns.SDK_ENDPOINT.builtIn

private fun configParamNewtype(
    parameter: Parameter,
    name: String,
    runtimeConfig: RuntimeConfig,
): RuntimeType {
    val type = parameter.symbol().mapRustType { t -> t.stripOuter<RustType.Option>() }
    return when (promotedBuiltins(parameter)) {
        true ->
            AwsRuntimeType.awsTypes(runtimeConfig)
                .resolve("endpoint_config::${name.toPascalCase()}")

        false -> configParamNewtype(name.toPascalCase(), type, runtimeConfig)
    }
}

private fun ConfigParam.Builder.toConfigParam(
    parameter: Parameter,
    runtimeConfig: RuntimeConfig,
): ConfigParam =
    this.name(this.name ?: parameter.name.rustName())
        .type(parameter.symbol().mapRustType { t -> t.stripOuter<RustType.Option>() })
        .newtype(configParamNewtype(parameter, this.name!!, runtimeConfig))
        .setterDocs(this.setterDocs ?: parameter.documentation.orNull()?.let { writable { docs(it) } })
        .build()

fun Model.loadBuiltIn(
    serviceId: ShapeId,
    builtInSrc: Parameter,
): Parameter? {
    val model = this
    val idx = EndpointRulesetIndex.of(model)
    val service = model.expectShape(serviceId, ServiceShape::class.java)
    val rules = idx.endpointRulesForService(service) ?: return null
    // load the builtIn with a matching name from the ruleset allowing for any docs updates
    return rules.getBuiltIn(builtInSrc)
}

fun Model.sdkConfigSetter(
    codegenContext: ClientCodegenContext,
    builtInSrc: Parameter,
    configParameterNameOverride: String?,
): AdHocCustomization? {
    val serviceId = codegenContext.serviceShape.id
    val builtIn = loadBuiltIn(serviceId, builtInSrc) ?: return null
    val fieldName = configParameterNameOverride ?: builtIn.name.rustName()

    return adhocCustomization<ServiceConfigSection.MergeFromSharedConfig> { section ->
        val mapToOwned =
            when (val builtinType = builtIn.type!!) {
                ParameterType.STRING -> writable { rust("|s|s.to_string()") }
                ParameterType.BOOLEAN -> null
                // No builtins currently toOwned to stringArray
                else -> PANIC("needs to handle unimplemented endpoint parameter builtin type: $builtinType")
            }
        val mapToOwnedBlock = mapToOwned?.let { writable { rust(".map(#W)", it) } } ?: writable { }
        val newType = configParamNewtype(builtInSrc, fieldName, codegenContext.runtimeConfig)
        rustTemplate(
            """
            if self.field_never_set::<#{newType}>() {
                self.set_$fieldName(${section.sdkConfig}.$fieldName()#{map_to_owned});
            }
            """,
            "newType" to newType,
            "map_to_owned" to mapToOwnedBlock,
        )
    }
}

/**
 * Create a client codegen decorator that creates bindings for a builtIn parameter. Optionally, you can provide
 * [clientParam.Builder] which allows control over the config parameter that will be generated.
 */
fun decoratorForBuiltIn(
    builtIn: Parameter,
    clientParamBuilder: ConfigParam.Builder? = null,
): ClientCodegenDecorator {
    val nameOverride = clientParamBuilder?.name
    val name = nameOverride ?: builtIn.name.rustName()
    return object : ClientCodegenDecorator {
        override val name: String = "Auto${builtIn.builtIn.get()}"
        override val order: Byte = 0

        private fun rulesetContainsBuiltIn(codegenContext: ClientCodegenContext) =
            codegenContext.getBuiltIn(builtIn) != null

        override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> =
            listOfNotNull(
                codegenContext.model.sdkConfigSetter(
                    codegenContext,
                    builtIn,
                    clientParamBuilder?.name,
                ),
            ).extendIf(name == "endpoint_url") {
                adhocCustomization<ServiceConfigSection.LoadFromServiceSpecificEnv> { _ ->
                    val runtimeConfig = codegenContext.runtimeConfig
                    val serviceId = codegenContext.serviceShape.sdkId()
                    rustTemplate(
                        """
                        if self.field_never_set::<#{EndpointUrl}>() &&
                            !self.explicitly_set_in_shared_config("endpoint_url")
                        {
                            let endpoint_url = #{LoadServiceConfig}::load_config(
                                &env_config_loader, service_config_key(
                                    ${serviceId.dq()},
                                    "AWS_ENDPOINT_URL",
                                    "endpoint_url",
                                ))
                                .and_then(|it| it.parse::<#{Url}>().ok());
                            endpoint_url.map(|url| self.set_endpoint_url(#{Some}(url.to_string())));
                        }
                        """,
                        *preludeScope,
                        "EndpointUrl" to AwsRuntimeType.awsTypes(runtimeConfig).resolve("endpoint_config::EndpointUrl"),
                        "LoadServiceConfig" to
                            AwsRuntimeType.awsTypes(runtimeConfig)
                                .resolve("service_config::LoadServiceConfig"),
                        "Url" to RuntimeType.Url.resolve("Url"),
                    )
                }
            }

        override fun configCustomizations(
            codegenContext: ClientCodegenContext,
            baseCustomizations: List<ConfigCustomization>,
        ): List<ConfigCustomization> {
            return baseCustomizations.extendIf(rulesetContainsBuiltIn(codegenContext)) {
                standardConfigParam(
                    clientParamBuilder?.toConfigParam(builtIn, codegenContext.runtimeConfig) ?: ConfigParam.Builder()
                        .toConfigParam(builtIn, codegenContext.runtimeConfig),
                )
            }
        }

        override fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> =
            listOf(
                object : EndpointCustomization {
                    override fun loadBuiltInFromServiceConfig(
                        parameter: Parameter,
                        configRef: String,
                    ): Writable? =
                        when (parameter.builtIn) {
                            builtIn.builtIn ->
                                writable {
                                    val newtype = configParamNewtype(parameter, name, codegenContext.runtimeConfig)
                                    val symbol = parameter.symbol().mapRustType { t -> t.stripOuter<RustType.Option>() }
                                    rustTemplate(
                                        """$configRef.#{load_from_service_config_layer}""",
                                        "load_from_service_config_layer" to loadFromConfigBag(symbol.name, newtype),
                                    )
                                }

                            else -> null
                        }

                    override fun setBuiltInOnServiceConfig(
                        name: String,
                        value: Node,
                        configBuilderRef: String,
                    ): Writable? {
                        if (name != builtIn.builtIn.get()) {
                            return null
                        }
                        return writable {
                            rustTemplate(
                                "let $configBuilderRef = $configBuilderRef.${nameOverride ?: builtIn.name.rustName()}(#{value});",
                                "value" to value.toWritable(),
                            )
                        }
                    }
                },
            )
    }
}

private val endpointUrlDocs =
    writable {
        rust(
            """
            /// Sets the endpoint URL used to communicate with this service.
            ///
            /// Note: this is used in combination with other endpoint rules, e.g. an API that applies a host-label prefix
            /// will be prefixed onto this URL. To fully override the endpoint resolver, use
            /// [`Builder::endpoint_resolver`].
            """.trimIndent(),
        )
    }

fun Node.toWritable(): Writable {
    val node = this
    return writable {
        when (node) {
            is StringNode -> rust(node.value.dq())
            is BooleanNode -> rust("${node.value}")
            else -> PANIC("unsupported value for a default: $node")
        }
    }
}

val PromotedBuiltInsDecorators =
    listOf(
        decoratorForBuiltIn(AwsBuiltIns.FIPS),
        decoratorForBuiltIn(AwsBuiltIns.DUALSTACK),
        decoratorForBuiltIn(
            BuiltIns.SDK_ENDPOINT,
            ConfigParam.Builder()
                .name("endpoint_url")
                .type(RuntimeType.String.toSymbol())
                .setterDocs(endpointUrlDocs),
        ),
        AccountIdEndpointModeBuiltInParamDecorator(),
        AccountIdBuiltInParamDecorator(),
    ).toTypedArray()
