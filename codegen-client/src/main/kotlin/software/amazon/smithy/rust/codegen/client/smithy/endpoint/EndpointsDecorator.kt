/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

import software.amazon.smithy.model.node.BooleanNode
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeType
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameter
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameters
import software.amazon.smithy.rulesengine.traits.ContextIndex
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.orNull

/**
 * BuiltInResolver enables potentially external codegen stages to provide sources for `builtIn` parameters.
 * For example, this allows AWS to provide the value for the region builtIn in separate codegen.
 *
 * If this resolver does not recognize the value, it MUST return `null`.
 */
interface EndpointCustomization {
    fun builtInDefaultValue(parameter: Parameter, configRef: String): Writable?
    fun customRuntimeFunctions(codegenContext: ClientCodegenContext): List<CustomRuntimeFunction>
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
            codegenContext.rootDecorator.endpointCustomizations(codegenContext),
        )
    }

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations + ClientContextDecorator(codegenContext) + EndpointConfigCustomization(
            codegenContext,
            codegenContext.rootDecorator.endpointCustomizations(codegenContext),
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
    private val ctx: ClientCodegenContext,
    private val operationShape: OperationShape,
    private val endpointCustomizations: List<EndpointCustomization>,
) :
    OperationCustomization() {

    private val runtimeConfig = ctx.runtimeConfig
    private val params =
        EndpointRulesetIndex.of(ctx.model).endpointRulesForService(ctx.serviceShape)?.parameters
    private val idx = ContextIndex.of(ctx.model)

    override fun section(section: OperationSection): Writable {
        // if we don't have any parameters, then we have no rules, don't bother
        if (params == null) {
            return emptySection
        }
        val codegenScope = arrayOf(
            "Params" to EndpointParamsGenerator(params).paramsStruct(),
            "BuildError" to runtimeConfig.operationBuildError(),
        )
        return when (section) {
            is OperationSection.MutateInput -> writable {
                rustTemplate(
                    """
                    let endpoint_params = #{Params}::builder()#{builderFields:W}.build()
                        .map_err(|err|#{BuildError}::other(err))?;
                    let endpoint_result = ${section.config}.endpoint_resolver.resolve_endpoint(&endpoint_params);
                    """,
                    "builderFields" to builderFields(params, section),
                    *codegenScope,
                )
            }

            is OperationSection.MutateRequest -> writable {
                // insert the endpoint resolution _result_ into the bag (note that this won't bail if endpoint
                // resolution failed)
                // this is temporary—in the long term, we will insert the endpoint into the bag directly, but this makes
                // it testable
                rustTemplate("${section.request}.properties_mut().insert(endpoint_params);")
                rustTemplate("${section.request}.properties_mut().insert(endpoint_result);")
            }

            else -> emptySection
        }
    }

    private fun builderFields(params: Parameters, section: OperationSection.MutateInput) = writable {
        val memberParams = idx.getContextParams(operationShape)
        val builtInParams = params.toList().filter { it.isBuiltIn }
        // first load builtins and their defaults
        builtInParams.forEach { param ->
            val defaultProviders = endpointCustomizations.mapNotNull { it.builtInDefaultValue(param, section.config) }
            if (defaultProviders.size > 1) {
                error("Multiple providers provided a value for the builtin $param")
            }
            defaultProviders.firstOrNull()?.also { defaultValue ->
                rust(".set_${param.name.rustName()}(#W)", defaultValue)
            }
        }

        idx.getClientContextParams(ctx.serviceShape).orNull()?.parameters?.forEach { (name, param) ->
            val paramName = EndpointParamsGenerator.memberName(name)
            val setterName = EndpointParamsGenerator.setterName(name)
            if (param.type == ShapeType.BOOLEAN) {
                rust(".$setterName(${section.config}.$paramName)")
            } else {
                rust(".$setterName(${section.config}.$paramName.clone())")
            }
        }

        idx.getStaticContextParams(operationShape).orNull()?.parameters?.forEach { (name, param) ->
            val setterName = EndpointParamsGenerator.setterName(name)
            val value = writable {
                when (val v = param.value) {
                    is BooleanNode -> rust("Some(${v.value})")
                    is StringNode -> rust("Some(${v.value.dq()}.to_string())")
                    else -> TODO("Unexpected static value type: $v")
                }
            }
            rust(".$setterName(#W)", value)
        }

        // lastly, allow these to be overridden by members
        memberParams.forEach { (memberShape, param) ->
            val memberName = ctx.symbolProvider.toMemberName(memberShape)
            rust(
                ".${EndpointParamsGenerator.setterName(param.name)}(${section.input}.$memberName.clone())",
            )
        }
    }
}
