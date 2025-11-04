/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators

import software.amazon.smithy.rulesengine.language.evaluation.type.BooleanType
import software.amazon.smithy.rulesengine.language.evaluation.type.OptionalType
import software.amazon.smithy.rulesengine.language.syntax.expressions.Reference
import software.amazon.smithy.rulesengine.language.syntax.expressions.functions.Coalesce
import software.amazon.smithy.rulesengine.language.syntax.expressions.functions.IsSet
import software.amazon.smithy.rulesengine.language.syntax.expressions.functions.LibraryFunction
import software.amazon.smithy.rulesengine.language.syntax.rule.Condition
import software.amazon.smithy.rulesengine.traits.EndpointBddTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.Context
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointsLib
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.Types
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen.ExpressionGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen.Ownership
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.unsafeToRustName
import software.amazon.smithy.rust.codegen.core.util.orNull

enum class RefType {
    Parameter,
    Variable,
}

data class VariableRef(
    val name: String,
    val refType: RefType,
    val isOptional: Boolean,
)

/**
 * Utility for generating condition evaluation code that returns a boolean.
 * Used by both rule-based and BDD-based endpoint resolvers.
 */
internal class ConditionEvaluationGenerator(
    private val codegenContext: ClientCodegenContext,
    stdlib: List<CustomRuntimeFunction>,
    endpointBddTrait: EndpointBddTrait,
) {
    private val parameters = endpointBddTrait.parameters
    private val runtimeConfig = codegenContext.runtimeConfig
    private val registry: FunctionRegistry = FunctionRegistry(stdlib)
    private val endpointResolverGenerator = EndpointResolverGenerator(codegenContext, stdlib)
    private val types = Types(runtimeConfig)
    private val codegenScope =
        arrayOf(
            "BoxError" to RuntimeType.boxError(runtimeConfig),
            "endpoint" to types.smithyHttpEndpointModule,
            "SmithyEndpoint" to types.smithyEndpoint,
            "EndpointFuture" to types.endpointFuture,
            "ResolveEndpointError" to types.resolveEndpointError,
            "EndpointError" to types.resolveEndpointError,
            "ServiceSpecificEndpointResolver" to codegenContext.serviceSpecificEndpointResolver(),
            "DiagnosticCollector" to EndpointsLib.DiagnosticCollector,
            *preludeScope,
        )

    private val allowLintsForResolver =
        listOf(
            // we generate if x { if y { if z { ... } } }
            "clippy::collapsible_if",
            // we generate `if (true) == expr { ... }`
            "clippy::bool_comparison",
            // we generate `if !(a == b)`
            "clippy::nonminimal_bool",
            // we generate `if x == "" { ... }`
            "clippy::comparison_to_empty",
            // we generate `if let Some(_) = ... { ... }`
            "clippy::redundant_pattern_matching",
            // we generate `if (s.as_ref() as &str) == ("arn:") { ... }`, and `s` can be either `String` or `&str`
            "clippy::useless_asref",
        )
    private val context = Context(registry, runtimeConfig)

    private fun annotateRefs(
        libraryFunction: LibraryFunction,
        annotatedRefs: MutableList<VariableRef>,
    ) {
        val argExprAndType = libraryFunction.arguments.zip(libraryFunction.functionDefinition.arguments)
        argExprAndType.forEachIndexed { index, (argExpr, argType) ->
            val isOptional = (argType is OptionalType)

            if (argExpr is Reference) {
                val refType =
                    if (parameters.toList().map { it.nameString }.contains(argExpr.name.toString())) {
                        RefType.Parameter
                    } else {
                        RefType.Variable
                    }
                annotatedRefs.add(VariableRef(argExpr.name.toString(), refType, isOptional))
            }
        }

        libraryFunction.arguments.forEachIndexed { index, arg ->
            if (arg is LibraryFunction) {
                annotateRefs(arg, annotatedRefs)
            }
        }
    }

    private fun generateOptionalParamChecks(condition: Condition): Pair<String, Array<Pair<String, Writable>>> {
        val annotatedRefs = mutableListOf<VariableRef>()
        annotateRefs(condition.function, annotatedRefs)

//        condition.toExpression().references.forEach { refName ->
//            val param = parameters.toList().find { it.name.toString() == refName }
//            val isOptional =
//                if (param != null) {
//                    annotatedRefs.add(VariableRef(refName, RefType.Parameter, isOptional))
//                } else {
//                    annotatedRefs.add(VariableRef(refName, RefType.Variable, isOptional))
//                }
//        }

        // TODO(Bdd): turn the annotated refs into an if/let block checking the tuple of
        annotatedRefs.forEach { it ->
        }

        val argExpressions =
            condition.function.arguments.mapIndexed { index, arg ->
                index to ExpressionGenerator(Ownership.Borrowed, context).generate(arg)
            }.toMap()

        val ifLetBlocks =
            annotatedRefs.map { index ->
                "if let Some(_param_$index) = #{arg_$index:W}"
            }.joinToString(" ")

        val templateArgs =
            argExpressions.map { (index, writable) ->
                "arg_$index" to writable
            }.toTypedArray()

        return ifLetBlocks to templateArgs
    }

    private fun generateConditionInternal(condition: Condition): Writable {
        return {
            val generator = ExpressionGenerator(Ownership.Borrowed, context)
            val fn = condition.targetFunction()

            println("LNJ CONDITION REFS: ${condition.function.references.map { it.unsafeToRustName() }}")

            // there are three patterns we need to handle:
            // 1. the RHS returns an option which we need to guard as "Some(...)"
            // 2. the RHS returns a boolean which we need to gate on
            // 3. the RHS is infallible (e.g. uriEncode)

            // (condition.result.orNull() ?: (fn as? Reference)?.name)?.rustName() ?: "_"

            // Check for optional parameters that need unwrapping
            val optionalParams = condition.function.functionDefinition.arguments.filter { it is OptionalType }
            val target = generator.generate(fn)

            val resultName = condition.result.map { it.toString().unsafeToRustName() }.orNull()
            val contextStore =
                if (resultName !== null) {
                    writable {
                        // TODO(bdd): might be better to use the functionDefinition.returnType here? But you can't get URL or ARN types by name
                        // they are just represented as a struct with fields, so would require a heuristic
                        val fnName = condition.function.name?.toString() ?: ""
                        when {
                            fnName == "parseURL" -> {
                                rustTemplate(
                                    "context.store(String::from(\"$resultName\"), ConditionResult::Url($resultName));",
                                )
                            }

                            fnName == "aws.partition" -> {
                                rustTemplate(
                                    "context.store(String::from(\"$resultName\"), ConditionResult::Partition($resultName));",
                                )
                            }

                            fnName == "aws.parseArn" -> {
                                rustTemplate(
                                    "context.store(String::from(\"$resultName\"), ConditionResult::Arn($resultName));",
                                )
                            }

                            condition.function.functionDefinition.returnType is BooleanType -> {
                                rustTemplate("context.store(String::from(\"$resultName\"), ConditionResult::Bool($resultName));")
                            }

                            else -> {
                                rustTemplate(
                                    "context.store(String::from(\"$resultName\"), ConditionResult::String($resultName.to_string()));",
                                )
                            }
                        }
                    }
                } else {
                    writable {}
                }

            val returnType = condition.function.functionDefinition.returnType

            val (ifLetBlocks, templateArgs) = generateOptionalParamChecks(condition)
            println("LNJ IFLET: $ifLetBlocks")
            println("LNJ TEMPLATE: ${templateArgs[0]}")
            when {
                // IsSet and Coalesce are inlined and not sourced from endpoint_lib
                condition.function is IsSet -> {
                    rustTemplate(
                        """
                        // LNJ IsSet
                        #{target:W}.is_some()
                        """.trimIndent(),
                        "target" to target,
                    )
                }

                condition.function is Coalesce -> {
                    rustTemplate(
                        """
                        // LNJ Coalesce
                        #{target:W}
                        """.trimIndent(),
                        "target" to target,
                    )
                }

                // Functions with optional parameters need parameter checking
                optionalParams.isNotEmpty() -> {
                    println("LNJ OPTIONAL PARAMS")

                    when {
                        returnType is OptionalType -> {
                            rustTemplate(
                                """$ifLetBlocks {
                                if let Some($resultName) = #{target:W} {
                                #{contextStore:W}
                                true
                                } else {
                                false
                                }
                                } else {
                                false
                                }""",
                                "target" to target,
                                "contextStore" to contextStore,
                                *templateArgs,
                            )
                        }

                        returnType is BooleanType -> {
                            rustTemplate(
                                """$ifLetBlocks {
                                if #{target:W} {
                                  #{contextStore:W}
                                  true
                                } else {
                                  #{contextStore:W}
                                  false
                                }
                                } else {
                                false
                                }""",
                                "target" to target,
                                "contextStore" to contextStore,
                                *templateArgs,
                            )
                        }

                        else -> {
                            rustTemplate(
                                """$ifLetBlocks {
                                let $resultName = #{target:W};
                                #{contextStore:W}
                                true
                                } else {
                                false
                                }""",
                                "target" to target,
                                "contextStore" to contextStore,
                                *templateArgs,
                            )
                        }
                    }
                }

                // Other functions
                returnType is OptionalType -> {
                    rustTemplate(
                        """if let Some($resultName) = #{target:W} {
                        #{contextStore:W}
                        true
                        } else {
                        false
                        }
                        """.trimMargin(),
                        "target" to target,
                        "contextStore" to contextStore,
                    )
                }

                returnType is BooleanType -> {
                    rustTemplate(
                        """
                        if #{target:W} {
                          #{contextStore:W}
                          true
                        } else {
                          #{contextStore:W}
                          false
                        }
                        """,
                        "target" to target,
                        "contextStore" to contextStore,
                    )
                }

                else -> {
                    // the function is infallible: just create a binding
                    rustTemplate(
                        """
                        {
                        // LNJ INFALLIBLE
                        let $resultName = #{target:W};
                        true
                        }
                        """,
                        "target" to generator.generate(fn),
                    )
                }
            }
        }
    }

    /**
     * Generate code that evaluates a condition and stores its result in context.
     * Returns a writable that evaluates to bool (true if result was Some, false otherwise).
     */
    fun generateCondition(
        condition: Condition,
        context: Context,
        conditionIndex: Int,
    ): Writable {
        return generateConditionInternal(condition)
    }

    /**
     * Deal with the actual target of the condition by flattening through isSet
     */
    private fun Condition.targetFunction() =
        when (val fn = this.function) {
            is IsSet -> fn.arguments[0]
            else -> fn
        }
}
