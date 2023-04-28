/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators

import software.amazon.smithy.model.node.BooleanNode
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeType
import software.amazon.smithy.model.traits.EndpointTrait
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameters
import software.amazon.smithy.rulesengine.traits.ContextIndex
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointTypesGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rustName
import software.amazon.smithy.rust.codegen.client.smithy.generators.EndpointTraitBindings
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.orNull

class EndpointParamsInterceptorGenerator(
    private val codegenContext: ClientCodegenContext,
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val endpointTypesGenerator = EndpointTypesGenerator.fromContext(codegenContext)
    private val codegenScope = codegenContext.runtimeConfig.let { rc ->
        val endpointTypesGenerator = EndpointTypesGenerator.fromContext(codegenContext)
        val runtimeApi = CargoDependency.smithyRuntimeApi(rc).toType()
        val interceptors = runtimeApi.resolve("client::interceptors")
        val orchestrator = runtimeApi.resolve("client::orchestrator")
        arrayOf(
            "BoxError" to runtimeApi.resolve("client::runtime_plugin::BoxError"),
            "ConfigBag" to runtimeApi.resolve("config_bag::ConfigBag"),
            "ContextAttachedError" to interceptors.resolve("error::ContextAttachedError"),
            "EndpointResolverParams" to orchestrator.resolve("EndpointResolverParams"),
            "HttpResponse" to orchestrator.resolve("HttpResponse"),
            "HttpRequest" to orchestrator.resolve("HttpRequest"),
            "Interceptor" to interceptors.resolve("Interceptor"),
            "InterceptorContext" to interceptors.resolve("InterceptorContext"),
            "InterceptorError" to interceptors.resolve("error::InterceptorError"),
            "ParamsBuilder" to endpointTypesGenerator.paramsBuilder(),
        )
    }

    fun render(writer: RustWriter, operationShape: OperationShape) {
        val operationName = symbolProvider.toSymbol(operationShape).name
        renderInterceptor(
            writer,
            "${operationName}EndpointParamsInterceptor",
            implInterceptorBodyForEndpointParams(operationShape),
        )
        renderInterceptor(
            writer, "${operationName}EndpointParamsFinalizerInterceptor",
            implInterceptorBodyForEndpointParamsFinalizer,
        )
    }

    private fun renderInterceptor(writer: RustWriter, interceptorName: String, implBody: Writable) {
        writer.rustTemplate(
            """
            ##[derive(Debug)]
            struct $interceptorName;

            impl #{Interceptor}<#{HttpRequest}, #{HttpResponse}> for $interceptorName {
                fn read_before_execution(
                    &self,
                    context: &#{InterceptorContext}<#{HttpRequest}, #{HttpResponse}>,
                    cfg: &mut #{ConfigBag},
                ) -> Result<(), #{BoxError}> {
                    #{body:W}
                }
            }
            """,
            *codegenScope,
            "body" to implBody,
        )
    }

    private fun implInterceptorBodyForEndpointParams(operationShape: OperationShape): Writable = writable {
        val operationInput = symbolProvider.toSymbol(operationShape.inputShape(model))
        rustTemplate(
            """
            // HACK: pull the handle out of the config bag until config is implemented right
            let handle = cfg.get::<std::sync::Arc<crate::client::Handle>>()
                .expect("the handle is hacked into the config bag");
            let config = &handle.conf;
            let input = context.input()?;
            let _input = input
                .downcast_ref::<${operationInput.name}>()
                .ok_or("failed to downcast to ${operationInput.name}")?;
            let params_builder = cfg
                .get::<#{ParamsBuilder}>()
                .ok_or("missing endpoint params builder")?
                .clone();
            cfg.put(params_builder
                #{param_setters}
            );

            #{endpoint_prefix:W}

            Ok(())
            """,
            *codegenScope,
            "endpoint_prefix" to endpointPrefix(operationShape),
            "param_setters" to paramSetters(operationShape, endpointTypesGenerator.params),
        )
    }

    private fun paramSetters(operationShape: OperationShape, params: Parameters) = writable {
        val idx = ContextIndex.of(codegenContext.model)
        val memberParams = idx.getContextParams(operationShape).toList().sortedBy { it.first.memberName }
        val builtInParams = params.toList().filter { it.isBuiltIn }
        // first load builtins and their defaults
        builtInParams.forEach { param ->
            endpointTypesGenerator.builtInFor(param, "config")?.also { defaultValue ->
                rust(".set_${param.name.rustName()}(#W)", defaultValue)
            }
        }

        idx.getClientContextParams(codegenContext.serviceShape).orNull()?.parameters?.forEach { (name, param) ->
            val paramName = EndpointParamsGenerator.memberName(name)
            val setterName = EndpointParamsGenerator.setterName(name)
            if (param.type == ShapeType.BOOLEAN) {
                rust(".$setterName(config.$paramName)")
            } else {
                rust(".$setterName(config.$paramName.clone())")
            }
        }

        idx.getStaticContextParams(operationShape).orNull()?.parameters?.forEach { (name, param) ->
            val setterName = EndpointParamsGenerator.setterName(name)
            val value = param.value.toWritable()
            rust(".$setterName(#W)", value)
        }

        // lastly, allow these to be overridden by members
        memberParams.forEach { (memberShape, param) ->
            val memberName = codegenContext.symbolProvider.toMemberName(memberShape)
            rust(
                ".${EndpointParamsGenerator.setterName(param.name)}(_input.$memberName.clone())",
            )
        }
    }

    private fun Node.toWritable(): Writable {
        val node = this
        return writable {
            when (node) {
                is StringNode -> rust("Some(${node.value.dq()}.to_string())")
                is BooleanNode -> rust("Some(${node.value})")
                else -> PANIC("unsupported default value: $node")
            }
        }
    }

    private fun endpointPrefix(operationShape: OperationShape): Writable = writable {
        operationShape.getTrait(EndpointTrait::class.java).map { epTrait ->
            val endpointTraitBindings = EndpointTraitBindings(
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
                    codegenContext.smithyRuntimeMode,
                )
            }
            rust("cfg.put(endpoint_prefix);")
        }
    }

    private val implInterceptorBodyForEndpointParamsFinalizer: Writable = writable {
        rustTemplate(
            """
            let _ = context;
            let params_builder = cfg
                .get::<#{ParamsBuilder}>()
                .ok_or("missing endpoint params builder")?
                .clone();
            let params = params_builder
                .build()
                .map_err(|err| #{ContextAttachedError}::new("endpoint params could not be built", err))?;
            cfg.put(
                #{EndpointResolverParams}::new(params)
            );

            Ok(())
            """,
            *codegenScope,
        )
    }
}
