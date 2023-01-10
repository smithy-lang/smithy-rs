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
import software.amazon.smithy.model.shapes.ShapeType
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rulesengine.language.EndpointRuleSet
import software.amazon.smithy.rulesengine.language.syntax.parameters.Builtins
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameter
import software.amazon.smithy.rulesengine.language.syntax.parameters.ParameterType
import software.amazon.smithy.rulesengine.traits.ClientContextParamDefinition
import software.amazon.smithy.rulesengine.traits.ClientContextParamsTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointCustomization
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointRulesetIndex
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rustName
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigParam
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.standardConfigParam
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocSection
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.extendIf
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.orNull

fun EndpointRuleSet.getBuiltIn(builtIn: Parameter) = parameters.toList().find { it.builtIn == builtIn.builtIn }
fun ClientCodegenContext.getBuiltIn(builtIn: Parameter): Parameter? {
    val idx = EndpointRulesetIndex.of(model)
    val rules = idx.endpointRulesForService(serviceShape) ?: return null
    return rules.getBuiltIn(builtIn)
}

/**
 * For legacy SDKs, there are builtIn parameters that cannot be automatically used as context parameters.
 *
 * However, for the Rust SDK, these parameters can be used directly.
 */
fun Model.promoteBuiltInToContextParam(serviceId: ShapeId, builtInSrc: Parameter): Model {
    val model = this
    // load the builtIn with a matching name from the ruleset allowing for any docs updates
    val builtIn = this.loadBuiltIn(serviceId, builtInSrc) ?: return model

    return model.addContextParam(
        serviceId, builtIn.name.asString(),
        ClientContextParamDefinition.builder().documentation(builtIn.documentation.get()).type(
            when (builtIn.type!!) {
                ParameterType.STRING -> ShapeType.STRING
                ParameterType.BOOLEAN -> ShapeType.BOOLEAN
            },
        ).build(),
    )
}

private fun toConfigParam(parameter: Parameter): ConfigParam = ConfigParam(
    parameter.name.rustName(),
    when (parameter.type!!) {
        ParameterType.STRING -> RuntimeType.String.toSymbol()
        ParameterType.BOOLEAN -> RuntimeType.Bool.toSymbol()
    },
    parameter.documentation.orNull(),
)

fun Model.addContextParam(
    serviceId: ShapeId,
    name: String,
    contextParamDefinition: ClientContextParamDefinition,
): Model {
    return ModelTransformer.create().mapShapes(this) { shape ->
        if (shape !is ServiceShape || shape.id != serviceId) {
            shape
        } else {
            val traitBuilder = shape.getTrait<ClientContextParamsTrait>()
                // there is a bug in the return type of the toBuilder method
                ?.let { ClientContextParamsTrait.builder().parameters(it.parameters) }
                ?: ClientContextParamsTrait.builder()
            val contextParamsTrait =
                traitBuilder.putParameter(
                    name,
                    contextParamDefinition,
                ).build()
            shape.toBuilder().removeTrait(ClientContextParamsTrait.ID).addTrait(contextParamsTrait).build()
        }
    }
}

fun Model.loadBuiltIn(serviceId: ShapeId, builtInSrc: Parameter): Parameter? {
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
    name: String?,
): Pair<AdHocSection<*>, (Section) -> Writable>? {
    val builtIn = loadBuiltIn(serviceId, builtInSrc) ?: return null
    val fieldName = name ?: builtIn.name.rustName()

    val map = when (builtIn.type!!) {
        ParameterType.STRING -> writable { rust("|s|s.to_string()") }
        ParameterType.BOOLEAN -> null
    }
    return SdkConfigSection.copyField(fieldName, map)
}

fun decoratorForBuiltIn(
    builtIn: Parameter,
    clientParam: ConfigParam? = null,
): ClientCodegenDecorator {
    val nameOverride = clientParam?.name
    val name = nameOverride ?: builtIn.name.rustName()
    return object : ClientCodegenDecorator {
        override val name: String = "Auto${builtIn.builtIn.get()}"
        override val order: Byte = 0

        private fun enabled(codegenContext: ClientCodegenContext) =
            codegenContext.model.loadBuiltIn(codegenContext.serviceShape.id, builtIn) != null

        override fun extraSections(codegenContext: ClientCodegenContext): List<Pair<AdHocSection<*>, (Section) -> Writable>> {
            return listOfNotNull(
                codegenContext.model.sdkConfigSetter(codegenContext.serviceShape.id, builtIn, clientParam?.name),
            )
        }

        override fun configCustomizations(
            codegenContext: ClientCodegenContext,
            baseCustomizations: List<ConfigCustomization>,
        ): List<ConfigCustomization> {
            return baseCustomizations.extendIf(enabled(codegenContext)) {
                standardConfigParam(
                    clientParam ?: toConfigParam(builtIn),
                )
            }
        }

        override fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> = listOf(
            object : EndpointCustomization {
                override fun builtInDefaultValue(parameter: Parameter, configRef: String): Writable? {
                    return if (parameter.builtIn == builtIn.builtIn) {
                        writable {
                            rust("$configRef.$name")
                            if (parameter.type == ParameterType.STRING) {
                                rust(".clone()")
                            }
                        }
                    } else {
                        null
                    }
                }

                override fun setBuiltInOnConfig(name: String, value: Node, configBuilderRef: String): Writable? {
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

private val endpointUrlDocs = """
    Sets the endpoint url used to communicate with this service

    Note: this is used in combination with other endpoint rules, e.g. an API that applies a host-label prefix
    will be prefixed onto this URL. To fully override the endpoint resolver, use
    [`Builder::endpoint_resolver`].
""".trimIndent()

private fun Node.toWritable(): Writable {
    val node = this
    return writable {
        when (node) {
            is StringNode -> rust(node.value.dq())
            is BooleanNode -> rust("${node.value}")
            else -> PANIC("unsupported default value: $node")
        }
    }
}

val PromotedBuiltInsDecorators =
    listOf(
        decoratorForBuiltIn(Builtins.FIPS),
        decoratorForBuiltIn(Builtins.DUALSTACK),
        decoratorForBuiltIn(
            Builtins.SDK_ENDPOINT,
            ConfigParam("endpoint_url", RuntimeType.String.toSymbol(), endpointUrlDocs),
        ),
    ).toTypedArray()
