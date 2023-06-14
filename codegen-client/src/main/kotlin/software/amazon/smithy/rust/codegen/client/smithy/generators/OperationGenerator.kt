/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.EndpointParamsInterceptorGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.MakeOperationGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.RequestSerializerGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ResponseDeserializerGenerator
import software.amazon.smithy.rust.codegen.client.smithy.protocols.HttpBoundProtocolTraitImplGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.derive
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.implBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolPayloadGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.outputShape

open class OperationGenerator(
    private val codegenContext: ClientCodegenContext,
    private val protocol: Protocol,
    /**
     * Operations generate a `make_operation(&config)` method to build a `aws_smithy_http::Operation` that can be dispatched
     * This is the serializer side of request dispatch
     */
    // TODO(enableNewSmithyRuntime): Remove the `makeOperationGenerator`
    private val makeOperationGenerator: MakeOperationGenerator,
    private val bodyGenerator: ProtocolPayloadGenerator,
    // TODO(enableNewSmithyRuntime): Remove the `traitGenerator`
    private val traitGenerator: HttpBoundProtocolTraitImplGenerator,
) {
    companion object {
        fun registerDefaultRuntimePluginsFn(runtimeConfig: RuntimeConfig): RuntimeType =
            RuntimeType.forInlineFun("register_default_runtime_plugins", ClientRustModule.Operation) {
                rustTemplate(
                    """
                    pub(crate) fn register_default_runtime_plugins(
                        runtime_plugins: #{RuntimePlugins},
                        operation: #{Box}<dyn #{RuntimePlugin} + #{Send} + #{Sync}>,
                        handle: #{Arc}<crate::client::Handle>,
                        config_override: #{Option}<crate::config::Builder>,
                    ) -> #{RuntimePlugins} {
                        let mut runtime_plugins = runtime_plugins
                            .with_client_plugin(crate::config::ServiceRuntimePlugin::new(handle))
                            .with_operation_plugin(operation);
                        if let Some(config_override) = config_override {
                            runtime_plugins = runtime_plugins.with_operation_plugin(config_override);
                        }
                        runtime_plugins
                    }
                    """,
                    *preludeScope,
                    "Arc" to RuntimeType.Arc,
                    "RuntimePlugin" to RuntimeType.runtimePlugin(runtimeConfig),
                    "RuntimePlugins" to RuntimeType.smithyRuntimeApi(runtimeConfig)
                        .resolve("client::runtime_plugin::RuntimePlugins"),
                )
            }
    }

    private val model = codegenContext.model
    private val runtimeConfig = codegenContext.runtimeConfig
    private val symbolProvider = codegenContext.symbolProvider

    /**
     * Render the operation struct and its supporting code.
     */
    fun renderOperation(
        operationWriter: RustWriter,
        // TODO(enableNewSmithyRuntime): Remove the `inputWriter` since `make_operation` generation is going away
        inputWriter: RustWriter,
        operationShape: OperationShape,
        codegenDecorator: ClientCodegenDecorator,
    ) {
        val operationCustomizations = codegenDecorator.operationCustomizations(codegenContext, operationShape, emptyList())
        val inputShape = operationShape.inputShape(model)

        // impl OperationInputShape { ... }
        inputWriter.implBlock(symbolProvider.toSymbol(inputShape)) {
            writeCustomizations(
                operationCustomizations,
                OperationSection.InputImpl(operationCustomizations, operationShape, inputShape, protocol),
            )
            if (codegenContext.smithyRuntimeMode.generateMiddleware) {
                makeOperationGenerator.generateMakeOperation(this, operationShape, operationCustomizations)
            }
        }

        renderOperationStruct(
            operationWriter,
            operationShape,
            operationCustomizations,
        )
    }

    private fun renderOperationStruct(
        operationWriter: RustWriter,
        operationShape: OperationShape,
        operationCustomizations: List<OperationCustomization>,
    ) {
        val operationName = symbolProvider.toSymbol(operationShape).name

        // pub struct Operation { ... }
        operationWriter.rust(
            """
            /// Orchestration and serialization glue logic for `$operationName`.
            """,
        )
        Attribute(derive(RuntimeType.Clone, RuntimeType.Default, RuntimeType.Debug)).render(operationWriter)
        Attribute.NonExhaustive.render(operationWriter)
        Attribute.DocHidden.render(operationWriter)
        operationWriter.rust("pub struct $operationName;")
        operationWriter.implBlock(symbolProvider.toSymbol(operationShape)) {
            Attribute.DocHidden.render(operationWriter)
            rustBlock("pub fn new() -> Self") {
                rust("Self")
            }

            val outputType = symbolProvider.toSymbol(operationShape.outputShape(model))
            val errorType = symbolProvider.symbolForOperationError(operationShape)
            val codegenScope = arrayOf(
                *preludeScope,
                "Arc" to RuntimeType.Arc,
                "Input" to symbolProvider.toSymbol(operationShape.inputShape(model)),
                "Operation" to symbolProvider.toSymbol(operationShape),
                "OperationError" to errorType,
                "OperationOutput" to outputType,
                "HttpResponse" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("client::orchestrator::HttpResponse"),
                "SdkError" to RuntimeType.sdkError(runtimeConfig),
            )
            if (codegenContext.smithyRuntimeMode.generateOrchestrator) {
                rustTemplate(
                    """
                    pub(crate) async fn orchestrate(
                        runtime_plugins: &#{RuntimePlugins},
                        input: #{Input},
                    ) -> #{Result}<#{OperationOutput}, #{SdkError}<#{OperationError}, #{HttpResponse}>> {
                        let map_err = |err: #{SdkError}<#{Error}, #{HttpResponse}>| {
                            err.map_service_error(|err| {
                                #{TypedBox}::<#{OperationError}>::assume_from(err.into())
                                    .expect("correct error type")
                                    .unwrap()
                            })
                        };
                        let context = Self::orchestrate_with_stop_point(&runtime_plugins, input, #{StopPoint}::None)
                            .await
                            .map_err(map_err)?;
                        let output = context.finalize().map_err(map_err)?;
                        #{Ok}(#{TypedBox}::<#{OperationOutput}>::assume_from(output).expect("correct output type").unwrap())
                    }

                    pub(crate) async fn orchestrate_with_stop_point(
                        runtime_plugins: &#{RuntimePlugins},
                        input: #{Input},
                        stop_point: #{StopPoint},
                    ) -> #{Result}<#{InterceptorContext}, #{SdkError}<#{Error}, #{HttpResponse}>> {
                        let input = #{TypedBox}::new(input).erase();
                        #{invoke_with_stop_point}(input, runtime_plugins, stop_point).await
                    }

                    pub(crate) fn register_runtime_plugins(
                        runtime_plugins: #{RuntimePlugins},
                        handle: #{Arc}<crate::client::Handle>,
                        config_override: #{Option}<crate::config::Builder>,
                    ) -> #{RuntimePlugins} {
                        #{register_default_runtime_plugins}(
                            runtime_plugins,
                            #{Box}::new(Self::new()) as _,
                            handle,
                            config_override
                        )
                        #{additional_runtime_plugins}
                    }
                    """,
                    *codegenScope,
                    "Error" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("client::interceptors::context::Error"),
                    "TypedBox" to RuntimeType.smithyTypes(runtimeConfig).resolve("type_erasure::TypedBox"),
                    "InterceptorContext" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("client::interceptors::InterceptorContext"),
                    "OrchestratorError" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("client::orchestrator::error::OrchestratorError"),
                    "RuntimePlugin" to RuntimeType.runtimePlugin(runtimeConfig),
                    "RuntimePlugins" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("client::runtime_plugin::RuntimePlugins"),
                    "StopPoint" to RuntimeType.smithyRuntime(runtimeConfig).resolve("client::orchestrator::StopPoint"),
                    "invoke_with_stop_point" to RuntimeType.smithyRuntime(runtimeConfig).resolve("client::orchestrator::invoke_with_stop_point"),
                    "register_default_runtime_plugins" to registerDefaultRuntimePluginsFn(runtimeConfig),
                    "additional_runtime_plugins" to writable {
                        writeCustomizations(
                            operationCustomizations,
                            OperationSection.AdditionalRuntimePlugins(operationCustomizations, operationShape),
                        )
                    },
                )
            }

            writeCustomizations(operationCustomizations, OperationSection.OperationImplBlock(operationCustomizations))
        }

        if (codegenContext.smithyRuntimeMode.generateMiddleware) {
            traitGenerator.generateTraitImpls(operationWriter, operationShape, operationCustomizations)
        }

        if (codegenContext.smithyRuntimeMode.generateOrchestrator) {
            OperationRuntimePluginGenerator(codegenContext).render(
                operationWriter,
                operationShape,
                operationName,
                operationCustomizations,
            )

            ResponseDeserializerGenerator(codegenContext, protocol)
                .render(operationWriter, operationShape, operationCustomizations)
            RequestSerializerGenerator(codegenContext, protocol, bodyGenerator)
                .render(operationWriter, operationShape)

            EndpointParamsInterceptorGenerator(codegenContext)
                .render(operationWriter, operationShape)
        }
    }
}
