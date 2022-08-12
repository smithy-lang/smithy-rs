/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
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
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.CodegenTarget
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.smithy.transformers.operationErrors
import software.amazon.smithy.rust.codegen.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.outputShape
import software.amazon.smithy.rust.codegen.util.toPascalCase

/**
 * ServerOperationHandlerGenerator
 */
open class ServerOperationHandlerGenerator(
    coreCodegenContext: CoreCodegenContext,
    private val operations: List<OperationShape>,
) {
    private val serverCrate = "aws_smithy_http_server"
    private val service = coreCodegenContext.serviceShape
    private val model = coreCodegenContext.model
    private val protocol = coreCodegenContext.protocol
    private val symbolProvider = coreCodegenContext.symbolProvider
    private val runtimeConfig = coreCodegenContext.runtimeConfig
    private val codegenScope = arrayOf(
        "AsyncTrait" to ServerCargoDependency.AsyncTrait.asType(),
        "Tower" to ServerCargoDependency.Tower.asType(),
        "FuturesUtil" to ServerCargoDependency.FuturesUtil.asType(),
        "SmithyHttp" to CargoDependency.SmithyHttp(runtimeConfig).asType(),
        "SmithyHttpServer" to ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType(),
        "Phantom" to ServerRuntimeType.Phantom,
        "ServerOperationHandler" to ServerRuntimeType.OperationHandler(runtimeConfig),
        "http" to RuntimeType.http,
    )

    open fun render(writer: RustWriter) {
        renderHandlerImplementations(writer, false)
        renderHandlerImplementations(writer, true)
    }

    /**
     * Renders the implementation of the `Handler` trait for all operations.
     * Handlers are implemented for `FnOnce` function types whose signatures take in state or not.
     */
    private fun renderHandlerImplementations(writer: RustWriter, state: Boolean) {
        operations.map { operation ->
            val operationName = symbolProvider.toSymbol(operation).name
            val inputName = symbolProvider.toSymbol(operation.inputShape(model)).fullName
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
                *codegenScope,
            ) {
                val callImpl = if (state) {
                    """
                    let state = match $serverCrate::extension::extract_extension(&mut req).await {
                        Ok(v) => v,
                        Err(extension_not_found_rejection) => {
                            let extension = $serverCrate::extension::RuntimeErrorExtension::new(extension_not_found_rejection.to_string());
                            let runtime_error = $serverCrate::runtime_error::RuntimeError {
                                protocol: #{SmithyHttpServer}::protocols::Protocol::${protocol.name.toPascalCase()},
                                kind: extension_not_found_rejection.into(),
                            };
                            let mut response = runtime_error.into_response();
                            response.extensions_mut().insert(extension);
                            return response.map($serverCrate::body::boxed);
                        }
                    };
                    let input_inner = input_wrapper.into();
                    let output_inner = self(input_inner, state).await;
                    """.trimIndent()
                } else {
                    """
                    let input_inner = input_wrapper.into();
                    let output_inner = self(input_inner).await;
                    """.trimIndent()
                }
                rustTemplate(
                    """
                    type Sealed = #{ServerOperationHandler}::sealed::Hidden;
                    async fn call(self, req: #{http}::Request<B>) -> #{http}::Response<#{SmithyHttpServer}::body::BoxBody> {
                        let mut req = #{SmithyHttpServer}::request::RequestParts::new(req);
                        let input_wrapper = match $inputWrapperName::from_request(&mut req).await {
                            Ok(v) => v,
                            Err(runtime_error) => {
                                return runtime_error.into_response().map($serverCrate::body::boxed);
                            }
                        };
                        $callImpl
                        let output_wrapper: $outputWrapperName = output_inner.into();
                        let mut response = output_wrapper.into_response();
                        let operation_ext = #{SmithyHttpServer}::extension::OperationExtension::new("${operation.id.namespace}##$operationName").expect("malformed absolute shape ID");
                        response.extensions_mut().insert(operation_ext);
                        response.map(#{SmithyHttpServer}::body::boxed)
                    }
                    """,
                    *codegenScope,
                )
            }
        }
    }

    /**
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
        val outputType = if (operation.operationErrors(model).isNotEmpty()) {
            "Result<${symbolProvider.toSymbol(operation.outputShape(model)).fullName}, ${operation.errorSymbol(model, symbolProvider, CodegenTarget.SERVER).fullyQualifiedName()}>"
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
            $serverCrate::rejection::RequestRejection: From<<B as $serverCrate::body::HttpBody>::Error>
        """.trimIndent()
    }
}
