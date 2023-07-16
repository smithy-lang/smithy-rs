/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

import software.amazon.smithy.model.node.BooleanNode
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeType
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameter
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameters
import software.amazon.smithy.rulesengine.traits.ContextIndex
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.CustomRuntimeFunction
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.EndpointParamsGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.endpointTestsModule
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen.SmithyEndpointsStdLib
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.orNull

/**
 * BuiltInResolver enables potentially external codegen stages to provide sources for `builtIn` parameters.
 * For example, this allows AWS to provide the value for the region builtIn in separate codegen.
 *
 * If this resolver does not recognize the value, it MUST return `null`.
 */
interface EndpointCustomization {
    /**
     * Provide the default value for [parameter] given a reference to the service config struct ([configRef])
     *
     * If this parameter is not recognized, return null.
     *
     * Example:
     * ```kotlin
     * override fun loadBuiltInFromServiceConfig(parameter: Parameter, configRef: String): Writable? {
     *     return when (parameter.builtIn) {
     *         Builtins.REGION.builtIn -> writable { rust("$configRef.region.as_ref().map(|r|r.as_ref().to_owned())") }
     *         else -> null
     *     }
     * }
     * ```
     */
    fun loadBuiltInFromServiceConfig(parameter: Parameter, configRef: String): Writable? = null

    /**
     * Set a given builtIn value on the service config builder. If this builtIn is not recognized, return null
     *
     * Example:
     * ```kotlin
     * override fun setBuiltInOnServiceConfig(name: String, value: Node, configBuilderRef: String): Writable? {
     *     if (name != Builtins.REGION.builtIn.get()) {
     *         return null
     *     }
     *     return writable {
     *         rustTemplate(
     *             "let $configBuilderRef = $configBuilderRef.region(#{Region}::new(${value.expectStringNode().value.dq()}));",
     *             "Region" to region(codegenContext.runtimeConfig).resolve("Region"),
     *         )
     *     }
     * }
     * ```
     */

    fun setBuiltInOnServiceConfig(name: String, value: Node, configBuilderRef: String): Writable? = null

    /**
     * Provide a list of additional endpoints standard library functions that rules can use
     */
    fun customRuntimeFunctions(codegenContext: ClientCodegenContext): List<CustomRuntimeFunction> = listOf()
}

/**
 * Decorator that injects endpoints 2.0 resolvers throughout the client.
 *
 * 1. Add ClientContext params to the config struct
 * 2. Inject params / endpoint results into the operation properties
 * 3. Set a default endpoint resolver (when available)
 * 4. Create an endpoint params structure/builder
 * 5. Generate endpoint tests (when available)
 *
 * This decorator installs the core standard library functions. It DOES NOT inject the AWS specific functions which
 * must be injected separately.
 *
 * If the service DOES NOT provide custom endpoint rules, this decorator is a no-op.
 */
class EndpointsDecorator : ClientCodegenDecorator {
    override val name: String = "Endpoints"
    override val order: Byte = 0

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        return baseCustomizations + InjectEndpointInMakeOperation(
            codegenContext,
            EndpointTypesGenerator.fromContext(codegenContext),
            operation,
        )
    }

    override fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> {
        return listOf(
            object : EndpointCustomization {
                override fun customRuntimeFunctions(codegenContext: ClientCodegenContext): List<CustomRuntimeFunction> {
                    return SmithyEndpointsStdLib
                }
            },
        )
    }

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations + ClientContextConfigCustomization(codegenContext) +
            EndpointConfigCustomization(codegenContext, EndpointTypesGenerator.fromContext(codegenContext))
    }

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        val generator = EndpointTypesGenerator.fromContext(codegenContext)
        rustCrate.withModule(ClientRustModule.endpoint(codegenContext)) {
            withInlineModule(endpointTestsModule(codegenContext), rustCrate.moduleDocProvider) {
                generator.testGenerator()(this)
            }
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
    // TODO(enableNewSmithyRuntimeCleanup): Delete this customization
    class InjectEndpointInMakeOperation(
        private val ctx: ClientCodegenContext,
        private val typesGenerator: EndpointTypesGenerator,
        private val operationShape: OperationShape,
    ) :
        OperationCustomization() {

        private val idx = ContextIndex.of(ctx.model)
        private val types = Types(ctx.runtimeConfig)

        override fun section(section: OperationSection): Writable {
            val codegenScope = arrayOf(
                *RuntimeType.preludeScope,
                "Params" to typesGenerator.paramsStruct(),
                "ResolveEndpoint" to types.resolveEndpoint,
                "ResolveEndpointError" to types.resolveEndpointError,
            )
            return when (section) {
                is OperationSection.MutateInput -> writable {
                    rustTemplate(
                        """
                        use #{ResolveEndpoint};
                        let params_result = #{Params}::builder()#{builderFields:W}.build()
                            .map_err(|err| #{ResolveEndpointError}::from_source("could not construct endpoint parameters", err));
                        let (endpoint_result, params) = match params_result {
                            #{Ok}(params) => (${section.config}.endpoint_resolver.resolve_endpoint(&params), #{Some}(params)),
                            #{Err}(e) => (#{Err}(e), #{None})
                        };
                        """,
                        "builderFields" to builderFields(typesGenerator.params, section),
                        *codegenScope,
                    )
                }

                is OperationSection.MutateRequest -> writable {
                    // insert the endpoint the bag
                    rustTemplate("${section.request}.properties_mut().insert(endpoint_result);")
                    rustTemplate("""if let #{Some}(params) = params { ${section.request}.properties_mut().insert(params); }""", *codegenScope)
                }

                else -> emptySection
            }
        }

        private fun Node.toWritable(): Writable {
            val node = this
            return writable {
                when (node) {
                    is StringNode -> rustTemplate("#{Some}(${node.value.dq()}.to_string())", *RuntimeType.preludeScope)
                    is BooleanNode -> rustTemplate("#{Some}(${node.value})", *RuntimeType.preludeScope)
                    else -> PANIC("unsupported default value: $node")
                }
            }
        }

        private fun builderFields(params: Parameters, section: OperationSection.MutateInput) = writable {
            val memberParams = idx.getContextParams(operationShape).toList().sortedBy { it.first.memberName }
            val builtInParams = params.toList().filter { it.isBuiltIn }
            // first load builtins and their defaults
            builtInParams.forEach { param ->
                typesGenerator.builtInFor(param, section.config)?.also { defaultValue ->
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
                val value = param.value.toWritable()
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
}
