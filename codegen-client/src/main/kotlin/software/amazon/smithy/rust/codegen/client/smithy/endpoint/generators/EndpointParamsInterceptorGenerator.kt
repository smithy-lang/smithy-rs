/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators

import software.amazon.smithy.jmespath.JmespathExpression
import software.amazon.smithy.model.node.ArrayNode
import software.amazon.smithy.model.node.BooleanNode
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.EndpointTrait
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameters
import software.amazon.smithy.rulesengine.traits.ContextIndex
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.ClientContextConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointTypesGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rustName
import software.amazon.smithy.rust.codegen.client.smithy.generators.EndpointTraitBindings
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.configParamNewtype
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.loadFromConfigBag
import software.amazon.smithy.rust.codegen.client.smithy.generators.waiters.RustJmespathShapeTraversalGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.waiters.TraversalBinding
import software.amazon.smithy.rust.codegen.client.smithy.generators.waiters.TraversalContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.waiters.TraversedShape
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.asRef
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.generators.enforceRequired
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.toPascalCase

class EndpointParamsInterceptorGenerator(
    private val codegenContext: ClientCodegenContext,
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val endpointTypesGenerator = EndpointTypesGenerator.fromContext(codegenContext)
    private val endpointCustomizations = codegenContext.rootDecorator.endpointCustomizations(codegenContext)
    private val codegenScope =
        codegenContext.runtimeConfig.let { rc ->
            val endpointTypesGenerator = EndpointTypesGenerator.fromContext(codegenContext)
            val runtimeApi = CargoDependency.smithyRuntimeApiClient(rc).toType()
            val interceptors = runtimeApi.resolve("client::interceptors")
            val orchestrator = runtimeApi.resolve("client::orchestrator")
            arrayOf(
                *preludeScope,
                "BoxError" to RuntimeType.boxError(rc),
                "ConfigBag" to RuntimeType.configBag(rc),
                "ContextAttachedError" to interceptors.resolve("error::ContextAttachedError"),
                "EndpointResolverParams" to runtimeApi.resolve("client::endpoint::EndpointResolverParams"),
                "HttpRequest" to orchestrator.resolve("HttpRequest"),
                "HttpResponse" to orchestrator.resolve("HttpResponse"),
                "Intercept" to RuntimeType.intercept(rc),
                "InterceptorContext" to RuntimeType.interceptorContext(rc),
                "BeforeSerializationInterceptorContextRef" to RuntimeType.beforeSerializationInterceptorContextRef(rc),
                "Input" to interceptors.resolve("context::Input"),
                "Output" to interceptors.resolve("context::Output"),
                "Error" to interceptors.resolve("context::Error"),
                "InterceptorError" to interceptors.resolve("error::InterceptorError"),
                "Params" to endpointTypesGenerator.paramsStruct(),
            )
        }

    fun render(
        writer: RustWriter,
        operationShape: OperationShape,
    ) {
        val operationName = symbolProvider.toSymbol(operationShape).name
        val operationInput = symbolProvider.toSymbol(operationShape.inputShape(model))
        val interceptorName = "${operationName}EndpointParamsInterceptor"
        writer.rustTemplate(
            """
            ##[derive(Debug)]
            struct $interceptorName;

            impl #{Intercept} for $interceptorName {
                fn name(&self) -> &'static str {
                    ${interceptorName.dq()}
                }

                fn read_before_execution(
                    &self,
                    context: &#{BeforeSerializationInterceptorContextRef}<'_, #{Input}, #{Output}, #{Error}>,
                    cfg: &mut #{ConfigBag},
                ) -> #{Result}<(), #{BoxError}> {
                    let _input = context.input()
                        .downcast_ref::<${operationInput.name}>()
                        .ok_or("failed to downcast to ${operationInput.name}")?;

                    #{endpoint_prefix:W}

                    let params = #{Params}::builder()
                        #{param_setters}
                        .build()
                        .map_err(|err| #{ContextAttachedError}::new("endpoint params could not be built", err))?;

                    #{track_sdk_features:W}

                    cfg.interceptor_state().store_put(#{EndpointResolverParams}::new(params));

                    #{Ok}(())
                }
            }

            // The get_* functions below are generated from JMESPath expressions in the
            // operationContextParams trait. They target the operation's input shape.

            #{jmespath_getters}
            """,
            *codegenScope,
            "endpoint_prefix" to endpointPrefix(operationShape),
            "param_setters" to paramSetters(operationShape, endpointTypesGenerator.params),
            "jmespath_getters" to jmesPathGetters(operationShape),
            "track_sdk_features" to trackSdkFeatures(),
        )
    }

    private fun paramSetters(
        operationShape: OperationShape,
        params: Parameters,
    ) = writable {
        val idx = ContextIndex.of(codegenContext.model)
        val memberParams = idx.getContextParams(operationShape).toList().sortedBy { it.first.memberName }
        val builtInParams = params.toList().filter { it.isBuiltIn }
        // first load builtins and their defaults
        builtInParams.forEach { param ->
            endpointTypesGenerator.builtInFor(param, "cfg")?.also { defaultValue ->
                rust(".set_${param.name.rustName()}(#W)", defaultValue)
            }
        }

        idx.getClientContextParams(codegenContext.serviceShape).orNull()?.parameters?.forEach { (name, param) ->
            val setterName = EndpointParamsGenerator.setterName(name)
            val inner = ClientContextConfigCustomization.toSymbol(param.type, symbolProvider)
            val newtype = configParamNewtype(name.toPascalCase(), inner, codegenContext.runtimeConfig)
            rustTemplate(
                ".$setterName(cfg.#{load_from_service_config_layer})",
                "load_from_service_config_layer" to loadFromConfigBag(inner.name, newtype),
            )
        }

        idx.getStaticContextParams(operationShape).orNull()?.parameters?.forEach { (name, param) ->
            val setterName = EndpointParamsGenerator.setterName(name)
            val value = param.value.toWritable()
            rust(".$setterName(#W)", value)
        }

        idx.getOperationContextParams(operationShape).orNull()?.parameters?.forEach { (name, param) ->
            val setterName = EndpointParamsGenerator.setterName(name)
            val getterName = EndpointParamsGenerator.getterName(name)
            val pathValue = param.path
            val pathExpression = JmespathExpression.parse(pathValue)
            val pathTraversal =
                RustJmespathShapeTraversalGenerator(codegenContext).generate(
                    pathExpression,
                    listOf(
                        TraversalBinding.Global(
                            "input",
                            TraversedShape.from(model, operationShape.inputShape(model)),
                        ),
                    ),
                    TraversalContext(retainOption = false),
                )

            when (pathTraversal.outputType) {
                is RustType.Vec -> {
                    if (pathTraversal.outputType.member is RustType.Reference) {
                        rust(".$setterName($getterName(_input).map(|v| v.into_iter().cloned().collect::<Vec<_>>()))")
                    } else {
                        rust(".$setterName($getterName(_input))")
                    }
                }
                else -> rust(".$setterName($getterName(_input).cloned())")
            }
        }

        // lastly, allow these to be overridden by members
        memberParams.forEach { (memberShape, param) ->
            val memberName = codegenContext.symbolProvider.toMemberName(memberShape)
            val member = memberShape.enforceRequired(writable("_input.$memberName.clone()"), codegenContext)

            rustTemplate(
                ".${EndpointParamsGenerator.setterName(param.name)}(#{member})", "member" to member,
            )
        }
    }

    private fun jmesPathGetters(operationShape: OperationShape) =
        writable {
            val idx = ContextIndex.of(codegenContext.model)
            val inputShape = operationShape.inputShape(codegenContext.model)
            val input = symbolProvider.toSymbol(inputShape)

            idx.getOperationContextParams(operationShape).orNull()?.parameters?.forEach { (name, param) ->
                val getterName = EndpointParamsGenerator.getterName(name)
                val pathValue = param.path
                val pathExpression = JmespathExpression.parse(pathValue)
                val pathTraversal =
                    RustJmespathShapeTraversalGenerator(codegenContext).generate(
                        pathExpression,
                        listOf(
                            TraversalBinding.Global(
                                "input",
                                TraversedShape.from(model, operationShape.inputShape(model)),
                            ),
                        ),
                        TraversalContext(retainOption = false),
                    )

                rust("// Generated from JMESPath Expression: $pathValue")
                rustBlockTemplate(
                    "fn $getterName(input: #{Input}) -> Option<#{Ret}>",
                    "Input" to input.rustType().asRef(),
                    "Ret" to pathTraversal.outputType,
                ) {
                    pathTraversal.output(this)
                    rust("Some(${pathTraversal.identifier})")
                }
            }
        }

    private fun Node.toWritable(): Writable {
        val node = this
        return writable {
            when (node) {
                is StringNode -> rust("Some(${node.value.dq()}.to_string())")
                is BooleanNode -> rust("Some(${node.value})")
                is ArrayNode -> {
                    // Cast the elements to a StringNode so this will fail if non-string values are provided
                    val elms = node.elements.map { "${(it as StringNode).value.dq()}.to_string()" }.joinToString(",")
                    rust("Some(vec![$elms])")
                }

                else -> PANIC("unsupported default value: $node")
            }
        }
    }

    private fun endpointPrefix(operationShape: OperationShape): Writable =
        writable {
            operationShape.getTrait(EndpointTrait::class.java).map { epTrait ->
                val endpointTraitBindings =
                    EndpointTraitBindings(
                        codegenContext.model,
                        symbolProvider,
                        codegenContext.runtimeConfig,
                        operationShape,
                        epTrait,
                    )
                withBlockTemplate(
                    "let endpoint_prefix = ",
                    """.map_err(|err| #{ContextAttachedError}::new("endpoint prefix could not be built", err))?;""",
                    *codegenScope,
                ) {
                    endpointTraitBindings.render(
                        this,
                        "_input",
                    )
                }
                rust("cfg.interceptor_state().store_put(endpoint_prefix);")
            }
        }

    private fun trackSdkFeatures() =
        writable {
            endpointCustomizations.mapNotNull {
                it.trackSdkFeatures(codegenContext, "cfg")
            }.forEach {
                rust("#T", it)
            }
        }
}
