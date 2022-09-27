/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoints

import software.amazon.smithy.model.node.BooleanNode
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeType
import software.amazon.smithy.rulesengine.language.syntax.parameters.Builtins
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameter
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameters
import software.amazon.smithy.rulesengine.traits.ContextIndex
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.core.util.orNull

/**
 * BuiltInResolver enables potentially external codegen stages to provide sources for `builtIn` parameters.
 * For example, allows AWS to provide the value for the region builtIn in separate codegen.
 *
 * If this resolver does not recognize the value, it MUST return `null`.
 */
interface RulesEngineBuiltInResolver {
    fun defaultFor(parameter: Parameter, configRef: String): Writable?
}

class EndpointsDecorator : RustCodegenDecorator<ClientProtocolGenerator, ClientCodegenContext> {
    override val name: String = "Endpoints"
    override val order: Byte = 0

    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean =
        clazz.isAssignableFrom(ClientCodegenContext::class.java)

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        return baseCustomizations + CreateEndpointParams(
            codegenContext,
            operation,
            codegenContext.rootDecorator.builtInResolvers(codegenContext),
        )
    }
}

/**
 * Creates an `<crate>::endpoint_resolver::Params` structure in make operation generator. This combines state from the
 * client, the operation, and the model to create parameters.
 *
 * Example generated code:
 * ```rust
 * let _endpoint_params = crate::endpoint_resolver::Params::builder()
 *     .set_region(Some("test-region"))
 *     .set_disable_everything(Some(true))
 *     .set_bucket(input.bucket.as_ref())
 *     .build();
 * ```
 */
class CreateEndpointParams(
    private val ctx: CodegenContext,
    private val operationShape: OperationShape,
    private val rulesEngineBuiltInResolvers: List<RulesEngineBuiltInResolver>,
) :
    OperationCustomization() {

    private val runtimeConfig = ctx.runtimeConfig
    private val params =
        EndpointRulesetIndex.of(ctx.model).endpointRulesForService(ctx.serviceShape)?.parameters
            ?: Parameters.builder().addParameter(Builtins.REGION).build()
    private val idx = ContextIndex.of(ctx.model)

    private val codegenScope = arrayOf(
        "Params" to EndpointParamsGenerator(params).paramsStruct(),
        "BuildError" to runtimeConfig.operationBuildError(),
    )

    override fun section(section: OperationSection): Writable {
        return when (section) {
            is OperationSection.MutateInput -> writable {
                // insert the endpoint resolution _result_ into the bag (note that this won't bail if endpoint
                // resolution failed)
                // generate with a leading `_` because we aren't generating rules that will use this for all services
                // yet.
                rustTemplate(
                    """
                    let _endpoint_params = #{Params}::builder()#{builderFields:W}.build();
                    """,
                    "builderFields" to builderFields(section),
                    *codegenScope,
                )
            }

            else -> emptySection
        }
    }

    private fun builderFields(section: OperationSection.MutateInput) = writable {
        val memberParams = idx.getContextParams(operationShape)
        val builtInParams = params.toList().filter { it.isBuiltIn }
        // first load builtins and their defaults
        builtInParams.forEach { param ->
            val defaultProviders = rulesEngineBuiltInResolvers.mapNotNull { it.defaultFor(param, section.config) }
            if (defaultProviders.size > 1) {
                error("Multiple providers provided a value for the builtin $param")
            }
            defaultProviders.firstOrNull()?.also { defaultValue ->
                rust(".set_${param.name.rustName()}(#W)", defaultValue)
            }
        }
        // NOTE(rcoh): we are not currently generating client context params onto the service shape yet
        // these can be overridden with client context params
        idx.getClientContextParams(ctx.serviceShape).orNull()?.parameters?.forEach { (name, param) ->
            val paramName = EndpointParamsGenerator.memberName(name)
            val setterName = EndpointParamsGenerator.setterName(name)
            if (param.type == ShapeType.BOOLEAN) {
                rust(".$setterName(${section.config}.$paramName)")
            } else {
                rust(".$setterName(${section.config}.$paramName.as_ref())")
            }
        }

        idx.getStaticContextParams(operationShape).orNull()?.parameters?.forEach { (name, param) ->
            val setterName = EndpointParamsGenerator.setterName(name)
            val value = writable {
                when (val v = param.value) {
                    is BooleanNode -> rust("Some(${v.value})")
                    is StringNode -> rust("Some(${v.value})")
                    else -> TODO("Unexpected static value type: $v")
                }
            }
            rust(".$setterName(#W)", value)
        }

        // lastly, allow these to be overridden by members
        memberParams.forEach { (memberShape, param) ->
            rust(
                ".${EndpointParamsGenerator.setterName(param.name)}(${section.input}.${
                ctx.symbolProvider.toMemberName(
                    memberShape,
                )
                }.as_ref())",
            )
        }
    }
}
