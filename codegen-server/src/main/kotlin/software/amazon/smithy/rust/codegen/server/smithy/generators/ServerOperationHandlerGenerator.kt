/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerRuntimeType
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerHttpBoundProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.outputShape
import software.amazon.smithy.rust.codegen.util.toPascalCase

/**
 * ServerOperationHandlerGenerator
 */
class ServerOperationHandlerGenerator(
    codegenContext: CodegenContext,
    private val operations: List<OperationShape>,
) {
    private val serverCrate = "aws_smithy_http_server"
    private val service = codegenContext.serviceShape
    private val model = codegenContext.model
    private val protocol = codegenContext.protocol
    private val symbolProvider = codegenContext.symbolProvider
    private val operationNames = operations.map { symbolProvider.toSymbol(it).name }
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope = arrayOf(
        "AsyncTrait" to ServerCargoDependency.AsyncTrait.asType(),
        "AxumCore" to ServerCargoDependency.AxumCore.asType(),
        "PinProjectLite" to ServerCargoDependency.PinProjectLite.asType(),
        "Tower" to ServerCargoDependency.Tower.asType(),
        "FuturesUtil" to ServerCargoDependency.FuturesUtil.asType(),
        "SmithyHttp" to CargoDependency.SmithyHttp(runtimeConfig).asType(),
        "SmithyHttpServer" to ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType(),
        "Phantom" to ServerRuntimeType.Phantom,
        "ServerOperationHandler" to ServerRuntimeType.serverOperationHandler(runtimeConfig),
        "Tracing" to ServerCargoDependency.Tracing.asType(),
        "http" to RuntimeType.http,
    )

    fun render(writer: RustWriter) {
        renderHandlerImplementations(writer, false)
        renderHandlerImplementations(writer, true)
    }

    /*
     * Renders the implementation of the `Handler` trait for all operations.
     * Handlers are implemented for `FnOnce` function types whose signatures take in state or not.
     */
    private fun renderHandlerImplementations(writer: RustWriter, state: Boolean) {
        operations.map { operation ->
            val operationName = symbolProvider.toSymbol(operation).name
            val inputName = "crate::input::${operationName}Input"
            val inputWrapperName = "crate::operation::$operationName${ServerHttpBoundProtocolGenerator.OPERATION_INPUT_WRAPPER_SUFFIX}"
            val outputWrapperName = "crate::operation::$operationName${ServerHttpBoundProtocolGenerator.OPERATION_OUTPUT_WRAPPER_SUFFIX}"
            val fnSignature = if (state) {
                "impl<B, Fun, Fut, S> #{ServerOperationHandler}::Handler<B, $serverCrate::Extension<S>, $inputName> for Fun"
            } else {
                "impl<B, Fun, Fut> #{ServerOperationHandler}::Handler<B, (), $inputName> for Fun"
            }
            writer.rustBlockTemplate(
                """
                ##[#{AsyncTrait}::async_trait]
                $fnSignature
                where
                    ${operationTraitBounds(operation, inputName, state)}
                """.trimIndent(),
                *codegenScope
            ) {
                // Instrument operation handler at callsite.
                val operationHandlerInvoke = if (state) {
                    "self(input_inner, state)"
                } else {
                    "self(input_inner)"
                }
                val operationHandlerCall =
                    """
                    let input_inner = input_wrapper.into();
                    #{Tracing}::debug!(input = ?input_inner, "calling operation handler");
                    let output_inner = $operationHandlerInvoke
                        .instrument(#{Tracing}::debug_span!("${operationName}_handler"))
                        .await;
                    #{Tracing}::debug!(output = ?output_inner, "operation handler returned");
                    """
                val callImpl = if (state) {
                    """
                    let state = match $serverCrate::extension::extract_extension(&mut req).await {
                        Ok(v) => v,
                        Err(extension_not_found_rejection) => {
                            #{Tracing}::error!(?extension_not_found_rejection, "unable to extract extension from request; maybe you forgot to register it with `AddExtensionLayer`?");
                            let extension = $serverCrate::extension::RuntimeErrorExtension::new(extension_not_found_rejection.to_string());
                            let runtime_error = $serverCrate::runtime_error::RuntimeError {
                                protocol: #{SmithyHttpServer}::protocols::Protocol::${protocol.name.toPascalCase()},
                                kind: extension_not_found_rejection.into(),
                            };
                            let mut response = runtime_error.into_response();
                            response.extensions_mut().insert(extension);
                            let response = response.map($serverCrate::body::boxed);
                            #{Tracing}::debug!(?response, "returning HTTP response");
                            return response;
                        }
                    };
                    $operationHandlerCall
                    """
                } else {
                    operationHandlerCall
                }
                rustTemplate(
                    """
                    type Sealed = #{ServerOperationHandler}::sealed::Hidden;
                    
                    ##[#{Tracing}::instrument(level = "debug", skip_all, name = "${operationName}_service_call")]
                    async fn call(self, req: #{http}::Request<B>) -> #{http}::Response<#{SmithyHttpServer}::body::BoxBody> {
                        use #{Tracing}::Instrument;
                    
                        let mut req = #{AxumCore}::extract::RequestParts::new(req);
                        use #{AxumCore}::extract::FromRequest;
                        use #{AxumCore}::response::IntoResponse;
                        let input_wrapper = match $inputWrapperName::from_request(&mut req).await {
                            Ok(v) => v,
                            Err(runtime_error) => {
                                #{Tracing}::debug!(?runtime_error, "unable to extract operation input from request");
                                let response = runtime_error.into_response().map($serverCrate::body::boxed);
                                #{Tracing}::debug!(?response, "returning HTTP response");
                                return response;
                            }
                        };
                        $callImpl
                        let output_wrapper: $outputWrapperName = output_inner.into();
                        let mut response = output_wrapper.into_response();
                        response.extensions_mut().insert(
                            #{SmithyHttpServer}::extension::OperationExtension::new("${operation.id.namespace}", "$operationName")
                        );
                        response = response.map(#{SmithyHttpServer}::body::boxed);
                        #{Tracing}::debug!(?response, "returning HTTP response");
                        response
                    }
                    """,
                    *codegenScope
                )
            }
        }
    }

    /*
     * Generates the trait bounds of the `Handler` trait implementation, depending on:
     *     - the presence of state; and
     *     - whether the operation is fallible or not.
     */
    private fun operationTraitBounds(operation: OperationShape, inputName: String, state: Boolean): String {
        val inputFn = if (state) {
            """S: Send + Clone + Sync + 'static,
            Fun: FnOnce($inputName, $serverCrate::Extension<S>) -> Fut + Clone + Send + 'static,"""
        } else {
            "Fun: FnOnce($inputName) -> Fut + Clone + Send + 'static,"
        }
        val outputType = if (operation.errors.isNotEmpty()) {
            "Result<${symbolProvider.toSymbol(operation.outputShape(model)).fullName}, ${operation.errorSymbol(symbolProvider).fullyQualifiedName()}>"
        } else {
            symbolProvider.toSymbol(operation.outputShape(model)).fullName
        }
        val streamingBodyTraitBounds = if (operation.inputShape(model).hasStreamingMember(model)) {
            "\n B: Into<#{SmithyHttp}::byte_stream::ByteStream>,"
        } else {
            ""
        }
        return """
            $inputFn
            Fut: std::future::Future<Output = $outputType> + Send,
            B: $serverCrate::body::HttpBody + Send + 'static, $streamingBodyTraitBounds
            B::Data: Send,
            B: std::fmt::Debug,
            $serverCrate::rejection::RequestRejection: From<<B as $serverCrate::body::HttpBody>::Error>
        """.trimIndent()
    }
}
