/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.client.smithy.generators.waiters

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.jmespath.JmespathExpression
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.replaceLifetimes
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.waiters.Matcher
import software.amazon.smithy.waiters.Matcher.ErrorTypeMember
import software.amazon.smithy.waiters.Matcher.InputOutputMember
import software.amazon.smithy.waiters.Matcher.OutputMember
import software.amazon.smithy.waiters.Matcher.SuccessMember
import software.amazon.smithy.waiters.PathComparator
import java.security.MessageDigest

private typealias Scope = Array<Pair<String, Any>>

/**
 * Generates the Rust code for the Smithy waiter "matcher union".
 * See https://smithy.io/2.0/additional-specs/waiters.html#matcher-union
 */
class RustWaiterMatcherGenerator(
    private val codegenContext: ClientCodegenContext,
    private val operationName: String,
    private val inputShape: Shape,
    private val outputShape: Shape,
) {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val module = RustModule.pubCrate("matchers", ClientRustModule.waiters)
    private val inputSymbol = codegenContext.symbolProvider.toSymbol(inputShape)
    private val outputSymbol = codegenContext.symbolProvider.toSymbol(outputShape)

    fun generate(
        errorSymbol: Symbol,
        matcher: Matcher<*>,
    ): RuntimeType {
        val fnName = fnName(operationName, matcher)
        val scope =
            arrayOf(
                *preludeScope,
                "Input" to inputSymbol,
                "Output" to outputSymbol,
                "Error" to errorSymbol,
                "ProvideErrorMetadata" to RuntimeType.provideErrorMetadataTrait(runtimeConfig),
            )
        return RuntimeType.forInlineFun(fnName, module) {
            docs("Matcher union: " + Node.printJson(matcher.toNode()))
            rustBlockTemplate("pub(crate) fn $fnName(_input: &#{Input}, _result: &#{Result}<#{Output}, #{Error}>) -> bool", *scope) {
                when (matcher) {
                    is OutputMember -> generateOutputMember(outputShape, matcher, scope)
                    is InputOutputMember -> generateInputOutputMember(matcher, scope)
                    is SuccessMember -> generateSuccessMember(matcher)
                    is ErrorTypeMember -> generateErrorTypeMember(matcher, scope)
                    else -> throw CodegenException("Unknown waiter matcher type: $matcher")
                }
            }
        }
    }

    private fun RustWriter.generateOutputMember(
        outputShape: Shape,
        matcher: OutputMember,
        scope: Scope,
    ) {
        val pathExpression = JmespathExpression.parse(matcher.value.path)
        val pathTraversal =
            RustJmespathShapeTraversalGenerator(codegenContext).generate(
                pathExpression,
                listOf(TraversalBinding.Global("_output", outputShape)),
            )

        generatePathTraversalMatcher(pathTraversal, matcher.value.expected, matcher.value.comparator, scope)
    }

    private fun RustWriter.generateInputOutputMember(
        matcher: InputOutputMember,
        scope: Scope,
    ) {
        val pathExpression = JmespathExpression.parse(matcher.value.path)
        val pathTraversal =
            RustJmespathShapeTraversalGenerator(codegenContext).generate(
                pathExpression,
                listOf(
                    TraversalBinding.Named("input", "_input", inputShape),
                    TraversalBinding.Named("output", "_output", outputShape),
                ),
            )

        generatePathTraversalMatcher(pathTraversal, matcher.value.expected, matcher.value.comparator, scope)
    }

    private fun RustWriter.generatePathTraversalMatcher(
        pathTraversal: GeneratedExpression,
        expected: String,
        comparatorKind: PathComparator,
        scope: Scope,
    ) {
        val comparator =
            writable {
                rust(
                    when (comparatorKind) {
                        PathComparator.ALL_STRING_EQUALS -> "value.iter().all(|s| s == ${expected.dq()})"
                        PathComparator.ANY_STRING_EQUALS -> "value.iter().any(|s| s == ${expected.dq()})"
                        PathComparator.STRING_EQUALS -> "value == ${expected.dq()}"
                        PathComparator.BOOLEAN_EQUALS ->
                            when (pathTraversal.outputType is RustType.Reference) {
                                true -> "*value == $expected"
                                else -> "value == $expected"
                            }
                        else -> throw CodegenException("Unknown path matcher comparator: $comparatorKind")
                    },
                )
            }

        rustTemplate(
            """
            fn path_traversal<'a>(_input: &'a #{Input}, _output: &'a #{Output}) -> #{Option}<#{TraversalOutput}> {
                #{traversal}
                #{Some}(${pathTraversal.identifier})
            }
            _result.as_ref()
                .ok()
                .and_then(|output| path_traversal(_input, output))
                .map(|value| #{comparator})
                .unwrap_or_default()
            """,
            *scope,
            "traversal" to pathTraversal.output,
            "TraversalOutput" to pathTraversal.outputType.replaceLifetimes("a"),
            "comparator" to comparator,
        )
    }

    private fun RustWriter.generateSuccessMember(matcher: SuccessMember) {
        rust(
            if (matcher.value) {
                "_result.is_ok()"
            } else {
                "_result.is_err()"
            },
        )
    }

    private fun RustWriter.generateErrorTypeMember(
        matcher: ErrorTypeMember,
        scope: Scope,
    ) {
        rustTemplate(
            """
            if let #{Err}(err) = _result {
                if let #{Some}(code) = #{ProvideErrorMetadata}::code(err) {
                    return code == ${matcher.value.dq()};
                }
            }
            false
            """,
            *scope,
        )
    }

    private fun fnName(
        operationName: String,
        matcher: Matcher<*>,
    ): String {
        // Smithy models don't give us anything useful to name these functions with, so just
        // SHA-256 hash the matcher JSON and truncate it to a reasonable length. This will have
        // a nice side-effect of de-duplicating identical matchers within a given operation.
        val jsonValue = Node.printJson(matcher.toNode())
        val bytes = MessageDigest.getInstance("SHA-256").digest(jsonValue.toByteArray())
        val hex = bytes.map { byte -> String.format("%02x", byte) }.joinToString("")
        return "match_${operationName.toSnakeCase()}_${hex.substring(0..16)}"
    }
}
