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
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointTypesGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.Types
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
import software.amazon.smithy.rust.codegen.core.smithy.mapRustType
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.extendIf
import software.amazon.smithy.rust.codegen.core.util.orNull
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
    serviceId: ShapeId,
    builtInSrc: Parameter,
    configParameterNameOverride: String?,
): AdHocCustomization? {
    val builtIn = loadBuiltIn(serviceId, builtInSrc) ?: return null
    val fieldName = configParameterNameOverride ?: builtIn.name.rustName()

    val builtinType = builtIn.type!!
    val map =
        when (builtinType) {
            ParameterType.STRING -> writable { rust("|s|s.to_string()") }
            ParameterType.BOOLEAN -> null
            // No builtins currently map to stringArray
            else -> PANIC("needs to handle unimplemented endpoint parameter builtin type: $builtinType")
        }

    return if (fieldName == "endpoint_url") {
        SdkConfigCustomization.copyFieldAndCheckForServiceConfig(fieldName, map)
    } else {
        SdkConfigCustomization.copyField(fieldName, map)
    }
}

/**
 * A custom decorator that creates bindings for the `accountID` built-in parameter.
 *
 * The `accountID` parameter is special because:
 * - It is not exposed in the `SdkConfig` setter.
 * - It does not require customizations like `loadBuiltInFromServiceConfig`,
 *  as it is not available when the `read_before_execution` method of the endpoint parameters interceptor is executed.
 */
class DecoratorForAccountId(private val builtIn: Parameter) : DecoratorForBuiltIn(builtIn) {
    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> {
        return emptyList()
    }

    override fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> =
        // TODO(AccountIdBasedRouting): Remove `builtIn == AwsBuiltIns.ACCOUNT_ID` once accountID becomes the only
        //  usage of this class
        if (rulesetContainsBuiltIn(codegenContext) && builtIn == AwsBuiltIns.ACCOUNT_ID) {
            listOf(
                object : EndpointCustomization {
                    override fun overrideResolveEndpointDefaultedTraitMethods(
                        codegenContext: ClientCodegenContext,
                    ): Writable? {
                        val runtimeConfig = codegenContext.runtimeConfig
                        return writable {
                            rustTemplate(
                                """
                                fn finalize_params<'a>(&'a self, params: &'a mut #{EndpointResolverParams}) -> #{FinalizeParamsFuture}<'a> {
                                    // This is required to satisfy the borrow checker. By obtaining an `Option<Identity>`,
                                    // `params` is no longer mutably borrowed in the match expression below.
                                    // Furthermore, by using `std::mem::replace` with an empty `Identity`, we avoid
                                    // leaving the sensitive `Identity` inside `params` within `EndpointResolverParams`.
                                    let identity = params
                                        .get_property_mut::<#{Identity}>()
                                        .map(|id| {
                                            std::mem::replace(
                                                id,
                                                #{Identity}::new((), #{None}),
                                            )
                                        });
                                    match (
                                        params.get_mut::<#{Params}>(),
                                        identity
                                            .as_ref()
                                            .and_then(|id| id.property::<#{AccountId}>()),
                                    ) {
                                        (#{Some}(concrete_params), #{Some}(account_id)) => {
                                            concrete_params.account_id = #{Some}(account_id.as_str().to_string());
                                        }
                                        (#{Some}(_), #{None}) => {
                                            // No account ID; nothing to do.
                                        }
                                        (#{None}, _) => {
                                            return #{FinalizeParamsFuture}::ready(
                                                #{Err}("service-specific endpoint params was not present".into()),
                                            );
                                        }
                                    }
                                    #{FinalizeParamsFuture}::ready(#{Ok}(()))
                                }
                                """,
                                *preludeScope,
                                *Types(runtimeConfig).toArray(),
                                "AccountId" to
                                    AwsRuntimeType.awsCredentialTypes(runtimeConfig)
                                        .resolve("attributes::AccountId"),
                                "FinalizeParamsFuture" to
                                    RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                                        .resolve("client::endpoint::FinalizeParamsFuture"),
                                "Identity" to
                                    RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                                        .resolve("client::identity::Identity"),
                                "Params" to EndpointTypesGenerator.fromContext(codegenContext).paramsStruct(),
                            )
                        }
                    }
                },
            )
        } else {
            emptyList()
        }
}

/**
 * A common client codegen decorator that creates bindings for a builtIn parameter. Optionally,
 * you can provide [clientParam.Builder] which allows control over the config parameter that will be generated.
 */
open class DecoratorForBuiltIn(
    private val builtIn: Parameter,
    private val clientParamBuilder: ConfigParam.Builder? = null,
) : ClientCodegenDecorator {
    override val name: String = "Auto${builtIn.builtIn.get()}"
    override val order: Byte = 0

    val builtinParamName = clientParamBuilder?.name ?: builtIn.name.rustName()

    protected fun rulesetContainsBuiltIn(codegenContext: ClientCodegenContext) =
        codegenContext.getBuiltIn(builtIn) != null

    override fun extraSections(codegenContext: ClientCodegenContext) =
        listOfNotNull(
            codegenContext.model.sdkConfigSetter(
                codegenContext.serviceShape.id,
                builtIn,
                clientParamBuilder?.name,
            ),
        )

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
                                val newtype =
                                    configParamNewtype(parameter, builtinParamName, codegenContext.runtimeConfig)
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
                            "let $configBuilderRef = $configBuilderRef.$builtinParamName(#{value});",
                            "value" to value.toWritable(),
                        )
                    }
                }
            },
        )
}

private val endpointUrlDocs =
    writable {
        rust(
            """
            /// Sets the endpoint URL used to communicate with this service

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
        DecoratorForBuiltIn(AwsBuiltIns.FIPS),
        DecoratorForBuiltIn(AwsBuiltIns.DUALSTACK),
        DecoratorForBuiltIn(
            BuiltIns.SDK_ENDPOINT,
            ConfigParam.Builder()
                .name("endpoint_url")
                .type(RuntimeType.String.toSymbol())
                .setterDocs(endpointUrlDocs),
        ),
        // TODO(AccountIdBasedRouting): Switch to DecoratorForBuiltIn once account_id_endpoint_mode is exposed
        //  in SdkConfig
        DecoratorForAccountId(AwsBuiltIns.ACCOUNT_ID_ENDPOINT_MODE),
        DecoratorForAccountId(AwsBuiltIns.ACCOUNT_ID),
    ).toTypedArray()
