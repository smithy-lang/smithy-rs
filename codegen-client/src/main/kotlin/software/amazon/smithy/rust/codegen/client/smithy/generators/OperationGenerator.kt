/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.EndpointParamsInterceptorGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.RequestSerializerGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ResponseDeserializerGenerator
import software.amazon.smithy.rust.codegen.client.smithy.protocols.ClientHttpBoundProtocolPayloadGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.derive
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.implBlock
import software.amazon.smithy.rust.codegen.core.rustlang.isNotEmpty
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolPayloadGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.sdkId

open class OperationGenerator(
    private val codegenContext: ClientCodegenContext,
    private val protocol: Protocol,
    private val bodyGenerator: ProtocolPayloadGenerator =
        ClientHttpBoundProtocolPayloadGenerator(
            codegenContext, protocol,
        ),
) {
    private val model = codegenContext.model
    private val runtimeConfig = codegenContext.runtimeConfig
    private val symbolProvider = codegenContext.symbolProvider

    /**
     * Render the operation struct and its supporting code.
     */
    fun renderOperation(
        operationWriter: RustWriter,
        operationShape: OperationShape,
        codegenDecorator: ClientCodegenDecorator,
    ) {
        val operationCustomizations =
            codegenDecorator.operationCustomizations(codegenContext, operationShape, emptyList())
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
        val serviceName = codegenContext.serviceShape.sdkId()

        // pub struct Operation { ... }
        operationWriter.rust(
            """
            /// Orchestration and serialization glue logic for `$operationName`.
            """,
        )
        Attribute(derive(RuntimeType.Clone, RuntimeType.Default, RuntimeType.Debug)).render(operationWriter)
        Attribute.NonExhaustive.render(operationWriter)
        operationWriter.rust("pub struct $operationName;")
        operationWriter.implBlock(symbolProvider.toSymbol(operationShape)) {
            docs("Creates a new `$operationName`")
            rustBlock("pub fn new() -> Self") {
                rust("Self")
            }

            val outputType = symbolProvider.toSymbol(operationShape.outputShape(model))
            val errorType = symbolProvider.symbolForOperationError(operationShape)
            val codegenScope =
                arrayOf(
                    *preludeScope,
                    "Arc" to RuntimeType.Arc,
                    "ConcreteInput" to symbolProvider.toSymbol(operationShape.inputShape(model)),
                    "Input" to
                        RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                            .resolve("client::interceptors::context::Input"),
                    "Operation" to symbolProvider.toSymbol(operationShape),
                    "OperationError" to errorType,
                    "OperationOutput" to outputType,
                    "HttpResponse" to
                        RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                            .resolve("client::orchestrator::HttpResponse"),
                    "SdkError" to RuntimeType.sdkError(runtimeConfig),
                )
            val additionalPlugins =
                writable {
                    writeCustomizations(
                        operationCustomizations,
                        OperationSection.AdditionalRuntimePlugins(operationCustomizations, operationShape),
                    )
                }
            val additionalSpanFields =
                writable {
                    writeCustomizations(
                        operationCustomizations,
                        OperationSection.AdditionalOperationSpanFields(operationCustomizations, operationShape),
                    )
                }
            rustTemplate(
                """
                pub(crate) async fn orchestrate(
                    runtime_plugins: &#{RuntimePlugins},
                    input: #{ConcreteInput},
                ) -> #{Result}<#{OperationOutput}, #{SdkError}<#{OperationError}, #{HttpResponse}>> {
                    let map_err = |err: #{SdkError}<#{Error}, #{HttpResponse}>| {
                        err.map_service_error(|err| {
                            err.downcast::<#{OperationError}>().expect("correct error type")
                        })
                    };
                    let context = Self::orchestrate_with_stop_point(runtime_plugins, input, #{StopPoint}::None)
                        .await
                        .map_err(map_err)?;
                    let output = context.finalize().map_err(map_err)?;
                    #{Ok}(output.downcast::<#{OperationOutput}>().expect("correct output type"))
                }

                pub(crate) async fn orchestrate_with_stop_point(
                    runtime_plugins: &#{RuntimePlugins},
                    input: #{ConcreteInput},
                    stop_point: #{StopPoint},
                ) -> #{Result}<#{InterceptorContext}, #{SdkError}<#{Error}, #{HttpResponse}>> {
                    let input = #{Input}::erase(input);
                    use #{Tracing}::Instrument;
                    #{invoke_with_stop_point}(
                        ${serviceName.dq()},
                        ${operationName.dq()},
                        input,
                        runtime_plugins,
                        stop_point
                    )
                    // Create a parent span for the entire operation. Includes a random, internal-only,
                    // seven-digit ID for the operation orchestration so that it can be correlated in the logs.
                    .instrument(#{Tracing}::debug_span!(
                            "$serviceName.$operationName",
                            "rpc.service" = ${serviceName.dq()},
                            "rpc.method" = ${operationName.dq()},
                            "sdk_invocation_id" = #{FastRand}::u32(1_000_000..10_000_000),
                            #{AdditionalSpanFields}
                        ))
                    .await
                }

                pub(crate) fn operation_runtime_plugins(
                    client_runtime_plugins: #{RuntimePlugins},
                    client_config: &crate::config::Config,
                    config_override: #{Option}<crate::config::Builder>,
                ) -> #{RuntimePlugins} {
                    let mut runtime_plugins = client_runtime_plugins.with_operation_plugin(Self::new());
                    #{additional_runtime_plugins}
                    if let #{Some}(config_override) = config_override {
                        for plugin in config_override.runtime_plugins.iter().cloned() {
                            runtime_plugins = runtime_plugins.with_operation_plugin(plugin);
                        }
                        runtime_plugins = runtime_plugins.with_operation_plugin(
                            crate::config::ConfigOverrideRuntimePlugin::new(config_override, client_config.config.clone(), &client_config.runtime_components)
                        );
                    }
                    runtime_plugins
                }
                """,
                *codegenScope,
                "Error" to
                    RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                        .resolve("client::interceptors::context::Error"),
                "InterceptorContext" to RuntimeType.interceptorContext(runtimeConfig),
                "OrchestratorError" to
                    RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                        .resolve("client::orchestrator::error::OrchestratorError"),
                "RuntimePlugin" to RuntimeType.runtimePlugin(runtimeConfig),
                "RuntimePlugins" to RuntimeType.runtimePlugins(runtimeConfig),
                "StopPoint" to RuntimeType.smithyRuntime(runtimeConfig).resolve("client::orchestrator::StopPoint"),
                "invoke_with_stop_point" to
                    RuntimeType.smithyRuntime(runtimeConfig)
                        .resolve("client::orchestrator::invoke_with_stop_point"),
                "additional_runtime_plugins" to
                    writable {
                        if (additionalPlugins.isNotEmpty()) {
                            rustTemplate(
                                """
                                runtime_plugins = runtime_plugins
                                    #{additional_runtime_plugins};
                                """,
                                "additional_runtime_plugins" to additionalPlugins,
                            )
                        }
                    },
                "Tracing" to RuntimeType.Tracing,
                "FastRand" to RuntimeType.FastRand,
                "AdditionalSpanFields" to additionalSpanFields,
            )

            writeCustomizations(operationCustomizations, OperationSection.OperationImplBlock(operationCustomizations))
        }

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
