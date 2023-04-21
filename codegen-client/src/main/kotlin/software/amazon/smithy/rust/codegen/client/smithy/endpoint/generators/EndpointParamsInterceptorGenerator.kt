/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.EndpointTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointTypesGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.EndpointTraitBindings
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.util.inputShape

class EndpointParamsInterceptorGenerator(
    private val codegenContext: ClientCodegenContext,
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val codegenScope = codegenContext.runtimeConfig.let { rc ->
        val endpointTypesGenerator = EndpointTypesGenerator.fromContext(codegenContext)
        val runtimeApi = CargoDependency.smithyRuntimeApi(rc).toType()
        val interceptors = runtimeApi.resolve("client::interceptors")
        val orchestrator = runtimeApi.resolve("client::orchestrator")
        arrayOf(
            "BoxError" to runtimeApi.resolve("client::runtime_plugin::BoxError"),
            "ConfigBag" to runtimeApi.resolve("config_bag::ConfigBag"),
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
            let input = context.input()?;
            let _input = input
                .downcast_ref::<${operationInput.name}>()
                .ok_or_else(|| #{InterceptorError}::invalid_input_access())?;
            let params_builder = cfg
                .get::<#{ParamsBuilder}>()
                .ok_or(#{InterceptorError}::read_before_execution("missing endpoint params builder"))?
                .clone();
            ${"" /* TODO(EndpointResolver): Call setters on `params_builder` to update its fields by using values from `_input` */}
            cfg.put(params_builder);

            #{endpoint_prefix:W}

            Ok(())
            """,
            *codegenScope,
            "endpoint_prefix" to endpointPrefix(operationShape),
        )
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
                ".map_err(#{InterceptorError}::read_before_execution)?;",
                *codegenScope,
            ) {
                endpointTraitBindings.render(
                    this,
                    "_input",
                    codegenContext.settings.codegenConfig.enableNewSmithyRuntime,
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
                .ok_or(#{InterceptorError}::read_before_execution("missing endpoint params builder"))?
                .clone();
            let params = params_builder
                .build()
                .map_err(#{InterceptorError}::read_before_execution)?;
            cfg.put(
                #{EndpointResolverParams}::new(params)
            );

            Ok(())
            """,
            *codegenScope,
        )
    }
}
