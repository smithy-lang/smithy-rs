/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerRuntimeType
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerHttpProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.outputShape

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
        "SmithyHttpServer" to CargoDependency.SmithyHttpServer(runtimeConfig).asType(),
        "SmithyRejection" to ServerHttpProtocolGenerator.smithyRejection(runtimeConfig),
        "Phantom" to ServerRuntimeType.Phantom,
        "ServerOperationHandler" to ServerRuntimeType.serverOperationHandler(runtimeConfig),
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
            val inputWrapperName = "crate::operation::$operationName${ServerHttpProtocolGenerator.OPERATION_INPUT_WRAPPER_SUFFIX}"
            val outputWrapperName = "crate::operation::$operationName${ServerHttpProtocolGenerator.OPERATION_OUTPUT_WRAPPER_SUFFIX}"
            val fnSignature = if (state) {
                "impl<B, Fun, Fut, S> #{ServerOperationHandler}::Handler<B, $serverCrate::Extension<S>, $inputName> for Fun"
            } else {
                "impl<B, Fun, Fut> #{ServerOperationHandler}::Handler<B, (), $inputName> for Fun"
            }
            val storeErrorInExtensions = """{
                let error = aws_smithy_http_server::ExtensionRejection::new(r.to_string());
                let mut response = r.into_response();
                response.extensions_mut().insert(error);
                return response.map($serverCrate::boxed);
                }
            """.trimIndent()
            writer.rustBlockTemplate(
                """
                ##[#{AsyncTrait}::async_trait]
                $fnSignature
                where
                    ${operationTraitBounds(operation, inputName, state)}
                """.trimIndent(),
                *codegenScope
            ) {
                val callImpl = if (state) {
                    """let state = match $serverCrate::Extension::<S>::from_request(&mut req).await {
                    Ok(v) => v,
                    Err(r) => $storeErrorInExtensions
                    };
                    let input_inner = input_wrapper.into();
                    let output_inner = self(input_inner, state).await;"""
                } else {
                    """let input_inner = input_wrapper.into();
                        let output_inner = self(input_inner).await;"""
                }
                rustTemplate(
                    """
                    type Sealed = #{ServerOperationHandler}::sealed::Hidden;
                    async fn call(self, req: #{http}::Request<B>) -> #{http}::Response<#{SmithyHttpServer}::BoxBody> {
                        let mut req = #{AxumCore}::extract::RequestParts::new(req);
                        use #{AxumCore}::extract::FromRequest;
                        use #{AxumCore}::response::IntoResponse;
                        let input_wrapper = match $inputWrapperName::from_request(&mut req).await {
                            Ok(v) => v,
                            Err(r) => $storeErrorInExtensions
                        };
                        $callImpl
                        let output_wrapper: $outputWrapperName = output_inner.into();
                        output_wrapper.into_response().map(#{SmithyHttpServer}::boxed)
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
            B: $serverCrate::HttpBody + Send + 'static, $streamingBodyTraitBounds
            B::Data: Send,
            B::Error: Into<$serverCrate::BoxError>,
            $serverCrate::rejection::SmithyRejection: From<<B as $serverCrate::HttpBody>::Error>
        """.trimIndent()
    }
}
