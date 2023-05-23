/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.protocol

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.EndpointParamsInterceptorGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationRuntimePluginGenerator
import software.amazon.smithy.rust.codegen.client.smithy.protocols.HttpBoundProtocolTraitImplGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.derive
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.implBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolPayloadGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.outputShape

open class ClientProtocolGenerator(
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
) : ProtocolGenerator(codegenContext, protocol) {
    private val runtimeConfig = codegenContext.runtimeConfig

    /**
     * Render all code required for serializing requests and deserializing responses for the operation
     *
     * This primarily relies on two components:
     * 1. [traitGenerator]: Generate implementations of the `ParseHttpResponse` trait for the operations
     * 2. [makeOperationGenerator]: Generate the `make_operation()` method which is used to serialize operations
     *    to HTTP requests
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

        renderOperationStruct(operationWriter, operationShape, operationCustomizations, codegenDecorator)
    }

    private fun renderOperationStruct(
        operationWriter: RustWriter,
        operationShape: OperationShape,
        operationCustomizations: List<OperationCustomization>,
        codegenDecorator: ClientCodegenDecorator,
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
                "RuntimePlugin" to RuntimeType.runtimePlugin(runtimeConfig),
                "RuntimePlugins" to RuntimeType.smithyRuntimeApi(runtimeConfig)
                    .resolve("client::runtime_plugin::RuntimePlugins"),
                "SdkError" to RuntimeType.sdkError(runtimeConfig),
            )
            if (codegenContext.smithyRuntimeMode.defaultToMiddleware) {
                rustTemplate(
                    """
                    // This is only used by paginators, and not all services have those
                    ##[allow(dead_code)]
                    pub(crate) async fn orchestrate(
                        input: #{Input},
                        handle: #{Arc}<crate::client::Handle>,
                        config_override: #{Option}<crate::config::Builder>,
                    ) -> #{Result}<#{OperationOutput}, #{SdkError}<#{OperationError}>> {
                        Self::orchestrate_with_middleware(input, handle, config_override).await
                    }
                    """,
                    *codegenScope,
                )
            } else {
                rustTemplate(
                    """
                    // This is only used by paginators, and not all services have those
                    ##[allow(dead_code)]
                    pub(crate) async fn orchestrate(
                        input: #{Input},
                        handle: #{Arc}<crate::client::Handle>,
                        config_override: #{Option}<crate::config::Builder>,
                    ) -> #{Result}<#{OperationOutput}, #{SdkError}<#{OperationError}, #{HttpResponse}>> {
                        Self::orchestrate_with_invoke(input, handle, config_override).await
                    }
                    """,
                    *codegenScope,
                )
            }
            if (codegenContext.smithyRuntimeMode.generateMiddleware) {
                rustTemplate(
                    """
                    ##[allow(unused_mut)]
                    pub(crate) async fn orchestrate_with_middleware(
                        mut input: #{Input},
                        handle: #{Arc}<crate::client::Handle>,
                        _config_override: #{Option}<crate::config::Builder>,
                    ) -> #{Result}<#{OperationOutput}, #{SdkError}<#{OperationError}>> {
                        let op = input.make_operation(&handle.conf).await.map_err(#{SdkError}::construction_failure)?;
                        handle.client.call(op).await
                    }
                    """,
                    *codegenScope,
                )
            }
            if (codegenContext.smithyRuntimeMode.generateOrchestrator) {
                val setupRuntimePluginsFn =
                    RuntimeType.forInlineFun("setup_runtime_plugins", ClientRustModule.Operation) {
                        rustTemplate(
                            """
                            pub(crate) fn setup_runtime_plugins(
                                operation: #{Box}<dyn #{RuntimePlugin} + #{Send} + #{Sync}>,
                                handle: #{Arc}<crate::client::Handle>,
                                config_override: #{Option}<crate::config::Builder>,
                            ) -> #{RuntimePlugins} {
                                let mut runtime_plugins = #{RuntimePlugins}::for_operation(operation)
                                    .with_client_plugin(crate::config::ServiceRuntimePlugin::new(handle));
                                if let Some(config_override) = config_override {
                                    runtime_plugins = runtime_plugins.with_operation_plugin(config_override);
                                }
                                runtime_plugins
                            }
                            """,
                            *codegenScope,
                        )
                    }
                rustTemplate(
                    """
                    pub(crate) async fn orchestrate_with_invoke(
                        input: #{Input},
                        handle: #{Arc}<crate::client::Handle>,
                        config_override: #{Option}<crate::config::Builder>,
                    ) -> #{Result}<#{OperationOutput}, #{SdkError}<#{OperationError}, #{HttpResponse}>> {
                        let runtime_plugins = #{setup_runtime_plugins}(#{Box}::new(#{Operation}::new()) as _, handle, config_override);
                        let input = #{TypedBox}::new(input).erase();
                        let output = #{invoke}(input, &runtime_plugins)
                            .await
                            .map_err(|err| {
                                err.map_service_error(|err| {
                                    #{TypedBox}::<#{OperationError}>::assume_from(err.into())
                                        .expect("correct error type")
                                        .unwrap()
                                })
                            })?;
                        #{Ok}(#{TypedBox}::<#{OperationOutput}>::assume_from(output).expect("correct output type").unwrap())
                    }
                    """,
                    *codegenScope,
                    "TypedBox" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("type_erasure::TypedBox"),
                    "invoke" to RuntimeType.smithyRuntime(runtimeConfig).resolve("client::orchestrator::invoke"),
                    "setup_runtime_plugins" to setupRuntimePluginsFn,
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
                codegenDecorator.operationRuntimePluginCustomizations(codegenContext, operationShape, emptyList()),
            )

            ResponseDeserializerGenerator(codegenContext, protocol)
                .render(operationWriter, operationShape, operationCustomizations)
            RequestSerializerGenerator(codegenContext, protocol, bodyGenerator)
                .render(operationWriter, operationShape, operationCustomizations)

            EndpointParamsInterceptorGenerator(codegenContext)
                .render(operationWriter, operationShape)
        }
    }
}
