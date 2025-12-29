/*
 *  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 *  SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen

import software.amazon.smithy.rulesengine.language.evaluation.type.BooleanType
import software.amazon.smithy.rulesengine.language.evaluation.type.OptionalType
import software.amazon.smithy.rulesengine.language.evaluation.type.Type
import software.amazon.smithy.rulesengine.language.syntax.Identifier
import software.amazon.smithy.rulesengine.language.syntax.expressions.Expression
import software.amazon.smithy.rulesengine.language.syntax.expressions.ExpressionVisitor
import software.amazon.smithy.rulesengine.language.syntax.expressions.Reference
import software.amazon.smithy.rulesengine.language.syntax.expressions.functions.FunctionDefinition
import software.amazon.smithy.rulesengine.language.syntax.expressions.functions.GetAttr
import software.amazon.smithy.rulesengine.language.syntax.expressions.functions.LibraryFunction
import software.amazon.smithy.rulesengine.language.syntax.expressions.literal.Literal
import software.amazon.smithy.rulesengine.language.syntax.expressions.literal.StringLiteral
import software.amazon.smithy.rulesengine.language.syntax.rule.Condition
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.Context
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.AnnotatedRefs
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.EndpointResolverGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rustName
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.conditionalBlock
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.PANIC
import java.lang.RuntimeException

/**
 * Root expression generator.
 */
class BddExpressionGenerator(
    private val condition: Condition,
    private val ownership: Ownership,
    private val context: Context,
    private val refs: AnnotatedRefs,
    private val codegenContext: ClientCodegenContext,
    private val knownSomeRefs: MutableSet<AnnotatedRefs.AnnotatedRef> = mutableSetOf(),
) {
    private val optionalRefNames = refs.filter { it.isOptional }.map { it.name }
    private val knownSomeRefsNames = knownSomeRefs.map { it.name }
    private val documentRefNames =
        refs.filter { it.runtimeType == RuntimeType.document(codegenContext.runtimeConfig) }.map { it.name }

    fun generateCondition(condition: Condition): Writable {
        return condition.function.accept(OuterExprGeneratorVisitor(ownership))
    }

    private fun generateExpression(expr: Expression): Writable {
        return expr.accept(
            ExprGeneratorVisitor(ownership),
        )
    }

    /**
     * Inner generator based on ExpressionVisitor
     */
    private open inner class ExprGeneratorVisitor(
        private val ownership: Ownership,
    ) :
        ExpressionVisitor<Writable> {
        override fun visitLiteral(literal: Literal): Writable {
            return literal.accept(LiteralGenerator(ownership, context))
        }

        override fun visitRef(ref: Reference) =
            writable {
                if (knownSomeRefsNames.contains(ref.name.rustName())) {
                    rust("""${ref.name.rustName()}.expect("${ref.name.rustName()} was set my a previous condition.")""")
                } else {
                    rust(ref.name.rustName())
                }
            }

        override fun visitGetAttr(getAttr: GetAttr): Writable {
            val target =
                BddExpressionGenerator(
                    condition,
                    ownership,
                    context,
                    refs,
                    codegenContext,
                    knownSomeRefs,
                ).generateExpression(
                    getAttr.target,
                )
            val targetIsOptionalRef =
                getAttr.target is Reference && optionalRefNames.contains((getAttr.target as Reference).name.rustName())

            val path =
                writable {
                    getAttr.path.toList().forEach { part ->
                        when (part) {
                            is GetAttr.Part.Key -> rust(".${part.key().rustName()}()")
                            is GetAttr.Part.Index -> {
                                if (part.index() == 0) {
                                    // In this case, `.first()` is more idiomatic and `.get(0)` triggers lint warnings
                                    rust(".first().cloned()")
                                } else {
                                    rust(".get(${part.index()}).cloned()") // we end up with Option<&&T>, we need to get to Option<&T>
                                }
                            }
                        }
                    }
                    if (ownership == Ownership.Owned) {
                        try {
                            if (getAttr.type() != Type.booleanType()) {
                                if (getAttr.type() is OptionalType) {
                                    rust(".map(|t|t.to_owned())")
                                } else {
                                    rust(".to_owned()")
                                }
                            }
                        } catch (_: RuntimeException) {
                            // Typechecking not available - default to .to_owned()
                            rust(".to_owned()")
                        }
                    }
                }
            return if (targetIsOptionalRef) {
                writable {
                    rustTemplate(
                        """
                        if let Some(inner) = #{Target} {
                            inner#{Path}
                        }else{
                            return false
                        }
                        """.trimIndent(),
                        "Target" to target, "Path" to path,
                    )
                }
            } else {
                writable { rust("#W#W", target, path) }
            }
        }

        override fun visitIsSet(fn: Expression) =
            writable {
                val expressionGenerator =
                    BddExpressionGenerator(condition, ownership, context, refs, codegenContext, knownSomeRefs)
                if (fn is Reference) {
                    // All references are in refs so safe to assert non-null
                    knownSomeRefs.add(refs.get(fn.name.rustName())!!)
                }
                rust("#W.is_some()", expressionGenerator.generateExpression(fn))
            }

        override fun visitNot(not: Expression) =
            writable {
                rust(
                    "!(#W)",
                    BddExpressionGenerator(
                        condition,
                        ownership,
                        context,
                        refs,
                        codegenContext,
                        knownSomeRefs,
                    ).generateExpression(not),
                )
            }

        override fun visitBoolEquals(
            left: Expression,
            right: Expression,
        ) = writable {
            val expressionGenerator =
                BddExpressionGenerator(condition, ownership, context, refs, codegenContext, knownSomeRefs)
            val lhsIsRef = left is Reference
            val rhsIsRef = right is Reference
            val lhsIsOptionalRef = (left is Reference && optionalRefNames.contains(left.name.rustName()))
            val rhsIsOptionalRef = (right is Reference && optionalRefNames.contains(right.name.rustName()))

            val rhsRef =
                if (lhsIsRef && !lhsIsOptionalRef && right is Literal) {
                    "&"
                } else {
                    ""
                }
            val lhsRef =
                if (rhsIsRef && !rhsIsOptionalRef && left is Literal) {
                    "&"
                } else {
                    ""
                }

            conditionalBlock("&mut Some(", ")", conditional = rhsIsOptionalRef) {
                rust(
                    "($lhsRef#W)",
                    expressionGenerator.generateExpression(left),
                )
            }

            rust("==")

            conditionalBlock("&mut Some(", ")", conditional = lhsIsOptionalRef) {
                rust(
                    "($rhsRef#W)",
                    expressionGenerator.generateExpression(right),
                )
            }
        }

        override fun visitStringEquals(
            left: Expression,
            right: Expression,
        ) = writable {
            val expressionGenerator =
                BddExpressionGenerator(condition, ownership, context, refs, codegenContext, knownSomeRefs)
            val lhsIsOptionalRef = (left is Reference && optionalRefNames.contains(left.name.rustName()))
            val rhsIsOptionalRef = (right is Reference && optionalRefNames.contains(right.name.rustName()))

            // References are always stored as String, but functions (GetAttr, coalesce, etc) often produce
            // &str, so we .into() those result to match the reference.
            val rhsInto =
                if (left is Reference && (right is Literal || right is GetAttr)) {
                    ".into()"
                } else {
                    ""
                }

            val lhsInto =
                if (right is Reference && (left is Literal || left is GetAttr)) {
                    ".into()"
                } else {
                    ""
                }

            conditionalBlock("&mut Some(", ")", conditional = rhsIsOptionalRef) {
                rust(
                    "(#W$lhsInto)",
                    expressionGenerator.generateExpression(left),
                )
            }

            rust("==")

            conditionalBlock("&mut Some(", ")", conditional = lhsIsOptionalRef) {
                rust(
                    "(#W$rhsInto)",
                    expressionGenerator.generateExpression(right),
                )
            }
        }

        override fun visitLibraryFunction(
            fn: FunctionDefinition,
            args: MutableList<Expression>,
        ): Writable =
            writable {
                val fnDefinition =
                    context.functionRegistry.fnFor(fn.id)
                        ?: PANIC(
                            "no runtime function for ${fn.id} " +
                                "(hint: if this is a custom or aws-specific runtime function, ensure the relevant standard library has been loaded " +
                                "on the classpath)",
                        )
                val expressionGenerator =
                    BddExpressionGenerator(condition, ownership, context, refs, codegenContext, knownSomeRefs)

//                if (fn.id == "coalesce") {
//                    val variadic = fn.variadicArguments.get()
//                    println("FunctionDefinition.id: ${fn.id}")
//                    println("FunctionDefinition.returnType: ${fn.returnType}")
//                    println("FunctionDefinition.arguments: ${fn.arguments}")
//                    println(
//                        "FunctionDefinition.variadicArguments: ${fn.variadicArguments.get() as Array<AnyType>}",
//                    )
//                    println("COALESCE args in visitLibraryFunction: $args")
//                }

                val argWritables =
                    args.map { arg ->
                        if (
                            (
                                (arg is Reference && optionalRefNames.contains(arg.name.rustName())) ||
                                    (
                                        arg is StringLiteral &&
                                            optionalRefNames.contains(
                                                Identifier.of(arg.value().toString()).rustName(),
                                            )
                                    ) ||
                                    (arg is LibraryFunction && arg.functionDefinition.returnType is OptionalType)
                            )
                        ) {
                            if ((fn.id == "coalesce" || fn.id == "ite") && !(arg is LibraryFunction && arg.functionDefinition.returnType is OptionalType)) {
                                writable {
                                    rust(
                                        """
                                        *#W
                                        """.trimMargin(),
                                        expressionGenerator.generateExpression(arg),
                                    )
                                }
                            } else {
                                val param =
                                    if (arg is Reference && documentRefNames.contains(arg.name.rustName())) {
                                        """&param.as_string().expect("Document was string type.")"""
                                    } else {
                                        "param"
                                    }

                                writable {
                                    rust(
                                        """
                                        if let Some(param) = #W{
                                            $param
                                        } else {
                                            return false
                                        }
                                        """.trimMargin(),
                                        expressionGenerator.generateExpression(arg),
                                    )
                                }
                            }
                        } else {
                            expressionGenerator.generateExpression(arg)
                        }
                    }

                // The macro stdlib fns don't take the diagnostic_collector
                if (fn.id == "coalesce" || fn.id == "ite") {
                    rustTemplate(
                        "#{fn}(#{args})",
                        "fn" to fnDefinition.usage(),
                        "args" to argWritables.join(","),
                    )
                } else {
                    rustTemplate(
                        "#{fn}(#{args}, ${EndpointResolverGenerator.DIAGNOSTIC_COLLECTOR})",
                        "fn" to fnDefinition.usage(),
                        "args" to argWritables.join(","),
                    )
                }

                if (ownership == Ownership.Owned) {
                    rust(".to_owned()")
                }
            }
    }

    /**
     * Outer expression generator visitor that handles cases that only happen in the
     * outermost evaluation, like assignment.
     */
    private inner class OuterExprGeneratorVisitor(
        private val ownership: Ownership,
    ) : ExprGeneratorVisitor(ownership) {
        override fun visitGetAttr(getAttr: GetAttr): Writable =
            writable {
                val innerExpressionGenerator =
                    BddExpressionGenerator(condition, ownership, context, refs, codegenContext, knownSomeRefs)
                val pathContainsIdx = getAttr.path.toList().filter { it is GetAttr.Part.Index }.isNotEmpty()

                // Condition performs an assignment to a field on ConditionContext
                if (condition.result.isPresent) {
                    // If the path contains an idx then the resulting type is Option<T>
                    if (pathContainsIdx) {
                        rustTemplate(
                            """
                            {
                                *${condition.result.get().rustName()} = #{FN:W}.map(|inner| inner.into());
                                ${condition.result.get().rustName()}.is_some()
                            }
                            """.trimIndent(),
                            "FN" to innerExpressionGenerator.generateExpression(condition.function),
                        )
                        // If no idx in path then getting the field is infallible
                    } else {
                        rustTemplate(
                            """
                            {
                                *${condition.result.get().rustName()} = Some(#{FN:W}.into());
                                true
                            }
                            """.trimIndent(),
                            "FN" to innerExpressionGenerator.generateExpression(condition.function),
                        )
                    }
                    // If no assignment performed just return the value
                } else {
                    rustTemplate(
                        """
                        #{FN:W}
                        ${if (pathContainsIdx) ".is_some()" else ""}
                        """.trimIndent(),
                        "FN" to innerExpressionGenerator.generateExpression(condition.function),
                    )
                }
            }

        override fun visitLibraryFunction(
            fn: FunctionDefinition,
            args: MutableList<Expression>,
        ): Writable =
            writable {
                val innerExpressionGenerator =
                    BddExpressionGenerator(condition, ownership, context, refs, codegenContext, knownSomeRefs)
                val fnReturnTypeOptional = condition.function.functionDefinition.returnType is OptionalType
                val fnReturnTypeIsBoolean = condition.function.functionDefinition.returnType is BooleanType

                // Condition performs an assignment to a field on ConditionContext
                if (condition.result.isPresent) {
                    // All results in the ConditionContext are Option<T> since they start out unassigned
                    if (!fnReturnTypeOptional && fnReturnTypeIsBoolean) {
                        rustTemplate(
                            """
                            {
                                let temp = #{FN:W}.into();
                                *${condition.result.get().rustName()} = Some(temp);
                                temp
                            }
                            """.trimIndent(),
                            "FN" to innerExpressionGenerator.generateExpression(condition.function),
                        )
                    } else if (!fnReturnTypeOptional && !fnReturnTypeIsBoolean) {
                        // Function is infallible, return true every time
                        rustTemplate(
                            """
                            {
                                *${condition.result.get().rustName()} = Some(#{FN:W}.into());
                                true
                            }
                            """.trimIndent(),
                            "FN" to innerExpressionGenerator.generateExpression(condition.function),
                        )
                    } else {
                        rustTemplate(
                            """
                            {
                                *${condition.result.get().rustName()} = #{FN:W}.map(|inner| inner.into());
                                ${condition.result.get().rustName()}.is_some()
                            }
                            """.trimIndent(),
                            "FN" to innerExpressionGenerator.generateExpression(condition.function),
                        )
                    }
                    // For optional types Some(_) is true and None is false
                } else if (!fnReturnTypeIsBoolean && fnReturnTypeOptional) {
                    rustTemplate(
                        """
                        (#{FN:W}).is_some()
                        """.trimIndent(),
                        "FN" to innerExpressionGenerator.generateExpression(condition.function),
                    )
                    // Just return the value
                } else {
                    rustTemplate(
                        """
                        #{FN:W}
                        """.trimIndent(),
                        "FN" to innerExpressionGenerator.generateExpression(condition.function),
                    )
                }
            }
    }
}
