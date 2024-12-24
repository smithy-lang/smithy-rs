/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.client.smithy.generators.waiters

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.waiters.AcceptorState
import software.amazon.smithy.waiters.Waiter

class WaiterAcceptorGenerator(
    private val codegenContext: ClientCodegenContext,
    operation: OperationShape,
    private val waiter: Waiter,
    private val inputName: String,
) {
    private val symbolProvider = codegenContext.symbolProvider
    private val runtimeConfig = codegenContext.runtimeConfig
    private val model = codegenContext.model

    private val operationName = symbolProvider.toSymbol(operation).name
    private val inputShape = operation.inputShape(model)
    private val outputShape = operation.outputShape(model)
    private val errorType = symbolProvider.symbolForOperationError(operation)

    private val scope =
        arrayOf(
            *preludeScope,
            "AcceptorState" to RuntimeType.smithyRuntime(runtimeConfig).resolve("client::waiters::AcceptorState"),
            "Output" to symbolProvider.toSymbol(outputShape),
            "Error" to errorType,
        )

    fun render(writer: RustWriter) {
        val matchers =
            waiter.acceptors.map { acceptor ->
                acceptor to
                    RustWaiterMatcherGenerator(codegenContext, operationName, inputShape, outputShape).generate(
                        errorType,
                        acceptor.matcher,
                    )
            }

        val anyRequiresInput = matchers.any { it.first.matcher.requiresInput() }
        val inputPrep =
            when {
                anyRequiresInput -> "let acceptor_input = $inputName.clone();"
                else -> ""
            }
        writer.rustTemplate(
            """
            $inputPrep
            let acceptor = move |result: #{Result}<&#{Output}, &#{Error}>| {
                #{acceptors}
            };
            """,
            *scope,
            "acceptors" to
                writable {
                    for ((acceptor, matcherFn) in matchers) {
                        val condition =
                            when {
                                acceptor.matcher.requiresInput() -> "#{matcher_fn}(&acceptor_input, result)"
                                else -> "#{matcher_fn}(result)"
                            }
                        val matcherComment = Node.printJson(acceptor.matcher.toNode())
                        val acceptorState = "#{AcceptorState}::${acceptor.state.rustName()}"
                        rustTemplate(
                            """
                            // Matches: $matcherComment
                            if $condition { return $acceptorState; }
                            """,
                            *scope,
                            "matcher_fn" to matcherFn,
                        )
                    }
                    rustTemplate("#{AcceptorState}::NoAcceptorsMatched", *scope)
                },
        )
    }

    private fun AcceptorState.rustName(): String =
        when (this) {
            AcceptorState.SUCCESS -> "Success"
            AcceptorState.FAILURE -> "Failure"
            AcceptorState.RETRY -> "Retry"
        }
}
