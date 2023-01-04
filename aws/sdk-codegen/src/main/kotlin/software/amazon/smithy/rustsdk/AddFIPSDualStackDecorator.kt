/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.Model
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
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointRulesetIndex
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rustName
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocSection
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.util.getTrait

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

    return ModelTransformer.create().mapShapes(model) { shape ->
        if (shape !is ServiceShape || shape.id != serviceId) {
            shape
        } else {
            val traitBuilder = shape.getTrait<ClientContextParamsTrait>()
                // there is a bug in the return type of the toBuilder method
                ?.let { ClientContextParamsTrait.builder().parameters(it.parameters) }
                ?: ClientContextParamsTrait.builder()
            val contextParamsTrait =
                traitBuilder.putParameter(
                    builtIn.name.asString(),
                    ClientContextParamDefinition.builder().documentation(builtIn.documentation.get()).type(
                        when (builtIn.type!!) {
                            ParameterType.STRING -> ShapeType.STRING
                            ParameterType.BOOLEAN -> ShapeType.BOOLEAN
                        },
                    ).build(),
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

fun Model.sdkConfigSetter(serviceId: ShapeId, builtInSrc: Parameter): Pair<AdHocSection<*>, (Section) -> Writable>? {
    val builtIn = loadBuiltIn(serviceId, builtInSrc) ?: return null
    val fieldName = builtIn.name.rustName()

    return SdkConfigSection.create { section ->
        {
            rust("${section.serviceConfigBuilder}.set_$fieldName(${section.sdkConfig}.$fieldName());")
        }
    }
}

class AddFIPSDualStackDecorator : ClientCodegenDecorator {
    override val name: String = "AddFipsDualStack"
    override val order: Byte = 0

    override fun transformModel(service: ServiceShape, model: Model): Model {
        return model
            .promoteBuiltInToContextParam(service.id, Builtins.FIPS)
            .promoteBuiltInToContextParam(service.id, Builtins.DUALSTACK)
    }

    override fun extraSections(codegenContext: ClientCodegenContext): List<Pair<AdHocSection<*>, (Section) -> Writable>> {
        return listOfNotNull(
            codegenContext.model.sdkConfigSetter(codegenContext.serviceShape.id, Builtins.FIPS),
            codegenContext.model.sdkConfigSetter(codegenContext.serviceShape.id, Builtins.DUALSTACK),
        )
    }
}
