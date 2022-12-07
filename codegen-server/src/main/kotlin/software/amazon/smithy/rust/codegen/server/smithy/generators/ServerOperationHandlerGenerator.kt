/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.core.smithy.transformers.operationErrors
import software.amazon.smithy.rust.codegen.core.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerRuntimeType
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerHttpBoundProtocolGenerator

/**
 * ServerOperationHandlerGenerator
 */
open class ServerOperationHandlerGenerator(
    codegenContext: CodegenContext,
    val protocol: ServerProtocol,
    private val operations: List<OperationShape>,
) {
    private val serverCrate = "aws_smithy_http_server"
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope = arrayOf(
        "AsyncTrait" to ServerCargoDependency.AsyncTrait.toType(),
        "Tower" to ServerCargoDependency.Tower.toType(),
        "FuturesUtil" to ServerCargoDependency.FuturesUtil.toType(),
        "SmithyHttp" to RuntimeType.smithyHttp(runtimeConfig),
        "SmithyHttpServer" to ServerCargoDependency.smithyHttpServer(runtimeConfig).toType(),
        "Phantom" to RuntimeType.Phantom,
        "ServerOperationHandler" to ServerRuntimeType.operationHandler(runtimeConfig),
        "http" to RuntimeType.Http,
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
                            let runtime_error = $serverCrate::runtime_error::RuntimeError::from(extension_not_found_rejection);
                            let mut response = #{SmithyHttpServer}::response::IntoResponse::<#{Protocol}>::into_response(runtime_error);
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
                                let response = #{SmithyHttpServer}::response::IntoResponse::<#{Protocol}>::into_response(runtime_error);
                                return response.map($serverCrate::body::boxed);
                            }
                        };
                        $callImpl
                        let output_wrapper: $outputWrapperName = output_inner.into();
                        let mut response = output_wrapper.into_response();
                        let operation_ext = #{SmithyHttpServer}::extension::OperationExtension::new("${operation.id.namespace}.$operationName").expect("malformed absolute shape ID");
                        response.extensions_mut().insert(operation_ext);
                        response.map(#{SmithyHttpServer}::body::boxed)
                    }
                    """,
                    "Protocol" to protocol.markerStruct(),
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
