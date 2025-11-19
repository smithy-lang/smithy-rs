/*
 *  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 *  SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen

import software.amazon.smithy.rulesengine.language.evaluation.type.BooleanType
import software.amazon.smithy.rulesengine.language.evaluation.type.OptionalType
import software.amazon.smithy.rulesengine.language.evaluation.type.Type
import software.amazon.smithy.rulesengine.language.syntax.expressions.Expression
import software.amazon.smithy.rulesengine.language.syntax.expressions.ExpressionVisitor
import software.amazon.smithy.rulesengine.language.syntax.expressions.Reference
import software.amazon.smithy.rulesengine.language.syntax.expressions.functions.FunctionDefinition
import software.amazon.smithy.rulesengine.language.syntax.expressions.functions.GetAttr
import software.amazon.smithy.rulesengine.language.syntax.expressions.functions.LibraryFunction
import software.amazon.smithy.rulesengine.language.syntax.expressions.literal.Literal
import software.amazon.smithy.rulesengine.language.syntax.rule.Condition
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.Context
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.AnnotatedRef
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.EndpointResolverGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.RefType
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.RustType
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rustName
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.plus
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.util.PANIC
import java.lang.RuntimeException

/**
 * Root expression generator.
 */
class BddExpressionGenerator(
    private val condition: Condition,
    private val ownership: Ownership,
    private val context: Context,
    private val refs: List<AnnotatedRef>,
    private val knownSomeRefs: MutableSet<AnnotatedRef>,
) {
    private val optionalRefNames = refs.filter { it.isOptional }.map { it.name }
    private val knownSomeRefsNames = knownSomeRefs.map { it.name }
    private val documentRefNames = refs.filter { it.rustType == RustType.Document }.map { it.name }

    fun generateCondition(
        condition: Condition,
        idx: Int,
    ): Writable {
        return condition.function.accept(OuterExprGeneratorVisitor(ownership, idx))
    }

    private fun generateExpression(
        expr: Expression,
        idx: Int,
    ): Writable {
        return expr.accept(
            ExprGeneratorVisitor(ownership, idx),
        )
    }

    /**
     * Inner generator based on ExprVisitor
     */
    private open inner class ExprGeneratorVisitor(
        private val ownership: Ownership,
        private val idx: Int,
    ) :
        ExpressionVisitor<Writable> {
        override fun visitLiteral(literal: Literal): Writable {
            return literal.accept(LiteralGenerator(ownership, context))
        }

        override fun visitRef(ref: Reference) =
            writable {
//                if (ownership == Ownership.Owned) {
//                    try {
//                        when (ref.type()) {
//
//                            is BooleanType -> {
//                                rust("// Owned typecheck Succeeded got BooleanType")
//                                rust("*${ref.name.rustName()}")
//                            }
//
//                            else -> {
//                                rust("// Owned typecheck Succeeded got not-BooleanType")
//                                rust("${ref.name.rustName()}.to_owned()")
//                            }
//                        }
//                    } catch (_: RuntimeException) {
//                        // Typechecking was never invoked - default to .to_owned()
//                        rust("// Owned typecheck failed")
//                        rust("${ref.name.rustName()}.to_owned()")
//                    }
//                } else {
//                    try {
//                        when (ref.type()) {
//                            // This ensures we obtain a `&str`, regardless of whether `ref.name.rustName()` returns a `String` or a `&str`.
//                            // Typically, we don't know which type will be returned due to code generation.
//                            is StringType -> {
//                                rust("// Borrowed typecheck succeeded got StringType")
//                                rust("${ref.name.rustName()}.as_ref() as &str")
//                            }
//
//                            else -> {
//                                rust("// Borrowed typecheck succeeded got not-StringType")
//                                rust(ref.name.rustName())
//                            }
//                        }
//                    } catch (_: RuntimeException) {
//                        // Because Typechecking was never invoked upon calling `.type()` on Reference for an expression
//                        // like "{ref}: rust". See `generateLiterals2` in ExprGeneratorTest.
//                        rust("// Borrowed typecheck failed")
//                        rust(ref.name.rustName())
//                    }
//                }

                if (knownSomeRefsNames.contains(ref.name.rustName())) {
                    rust("${ref.name.rustName()}.unwrap()")
                } else {
                    rust("${ref.name.rustName()}")
                }
            }

        override fun visitGetAttr(getAttr: GetAttr): Writable {
            val target =
                BddExpressionGenerator(condition, Ownership.Borrowed, context, refs, knownSomeRefs).generateExpression(
                    getAttr.target,
                    idx,
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
                // Smithy statically analyzes that the ref must have been set before this is used
                // so safe to unwrap
                val targetRustName = (getAttr.target as Reference).name.rustName()
//                writable { rust("""#W.as_ref().expect("$targetRustName should already be set")#W""", target, path) }
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
                    BddExpressionGenerator(condition, Ownership.Borrowed, context, refs, knownSomeRefs)
                if (fn is Reference) {
                    // All references are in refs so safe to assert non-null
                    knownSomeRefs.add(refs.find { it.name == fn.name.rustName() }!!)
                }
                rust("#W.is_some()", expressionGenerator.generateExpression(fn, idx))
            }

        override fun visitNot(not: Expression) =
            writable {
                rust(
                    "!(#W)",
                    BddExpressionGenerator(
                        condition,
                        Ownership.Borrowed,
                        context,
                        refs,
                        knownSomeRefs,
                    ).generateExpression(not, idx),
                )
            }

        override fun visitBoolEquals(
            left: Expression,
            right: Expression,
        ) = writable {
            val expressionGenerator = BddExpressionGenerator(condition, Ownership.Owned, context, refs, knownSomeRefs)
            val lhsIsRef = left is Reference
            val rhsIsRef = right is Reference
            val lhsIsOptionalRef = (left is Reference && optionalRefNames.contains(left.name.rustName()))
            val rhsIsOptionalRef = (right is Reference && optionalRefNames.contains(right.name.rustName()))
//            rust(
//                "(#W) == (#W)",
//                expressionGenerator.generateExpression(left, idx),
//                expressionGenerator.generateExpression(right, idx),
//            )

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

            if (rhsIsOptionalRef) {
                rust("&mut Some(")
            }
            rust(
                "($lhsRef#W)",
                expressionGenerator.generateExpression(left, idx),
            )
            if (rhsIsOptionalRef) {
                rust(")")
            }

            rust("==")

            if (lhsIsOptionalRef) {
                rust("&mut Some(")
            }
            rust(
                "($rhsRef#W)",
                expressionGenerator.generateExpression(right, idx),
            )
            if (lhsIsOptionalRef) {
                rust(")")
            }
        }

        override fun visitStringEquals(
            left: Expression,
            right: Expression,
        ) = writable {
            val expressionGenerator =
                BddExpressionGenerator(condition, Ownership.Borrowed, context, refs, knownSomeRefs)
            val lhsIsOptionalRef = (left is Reference && optionalRefNames.contains(left.name.rustName()))
            val rhsIsOptionalRef = (right is Reference && optionalRefNames.contains(right.name.rustName()))
            val lhsIsDocument = (
                left is Reference && refs.filter { it -> it.name == left.name.rustName() }
                    .get(0).rustType == RustType.Document
            )
            val rhsIsDocument = (
                right is Reference && refs.filter { it -> it.name == right.name.rustName() }
                    .get(0).rustType == RustType.Document
            )
            // Params are Option<String> while condition results are Option<&str>
            val lhsIsParameter = (
                left is Reference && refs.filter { it -> it.name == left.name.rustName() }
                    .get(0).refType == RefType.Parameter
            )
            val rhsIsParameter = (
                right is Reference && refs.filter { it -> it.name == right.name.rustName() }
                    .get(0).refType == RefType.Parameter
            )

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

            // TODO(BDD) this logic would probably look nicer with a writer util like conditionalBlock
            // but it is conditional parens and you can throw a Some (or other enum variant) in front
            if (rhsIsOptionalRef) {
                rust("&mut Some(")
            }
            rust(
                "(#W$lhsInto)",
                expressionGenerator.generateExpression(left, idx),
            )
            if (rhsIsOptionalRef) {
                rust(")")
            }

            rust("==")

            if (lhsIsOptionalRef) {
                rust("&mut Some(")
            }
            rust(
                "(#W$rhsInto)",
                expressionGenerator.generateExpression(right, idx),
            )
            if (lhsIsOptionalRef) {
                rust(")")
            }

//            rust(
//                "(#W) == (#W)",
//                expressionGenerator.generateExpression(left, idx),
//                expressionGenerator.generateExpression(right, idx),
//            )
        }

        override fun visitLibraryFunction(
            fn: FunctionDefinition,
            args: MutableList<Expression>,
        ): Writable =
            writable {
                // Special handling for coalesce - inline the logic since we can't do variadic fns
                if (fn.id == "coalesce") {
                    // Use Borrowed ownership to avoid type checks
                    val expressionGenerator =
                        BddExpressionGenerator(condition, Ownership.Borrowed, context, refs, knownSomeRefs)
                    val argWritables = args.map { expressionGenerator.generateExpression(it, idx) }

                    // TODO(BDD) I could probably update the macro to handle no inputs
                    if (argWritables.isEmpty()) {
                        rust("None")
                    } else {
                        rust("crate::coalesce!(")
                        for (i in 0 until argWritables.size) {
                            rust("#W,", argWritables[i])
                        }
                        rust(")")
                    }
                    return@writable
                }

                val fnDefinition =
                    context.functionRegistry.fnFor(fn.id)
                        ?: PANIC(
                            "no runtime function for ${fn.id} " +
                                "(hint: if this is a custom or aws-specific runtime function, ensure the relevant standard library has been loaded " +
                                "on the classpath)",
                        )
                val expressionGenerator =
                    BddExpressionGenerator(condition, Ownership.Borrowed, context, refs, knownSomeRefs)
                val argWritables =
                    args.map { arg ->
                        if ((arg is Reference && optionalRefNames.contains(arg.name.rustName())) ||
                            (arg is LibraryFunction && arg.functionDefinition.returnType is OptionalType)
                        ) {
                            val param =
                                if (arg is Reference && documentRefNames.contains(arg.name.rustName())) {
                                    "&param.as_string().unwrap()"
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
                                    expressionGenerator.generateExpression(arg, idx),
                                )
                            }
                        } else {
                            expressionGenerator.generateExpression(arg, idx)
                        }
                    }

//                var fnWriter = writable {
//                    rustTemplate(
//                        "#{fn}(#{args}, ${EndpointResolverGenerator.DIAGNOSTIC_COLLECTOR})",
//                        "fn" to fnDefinition.usage(),
//                        "args" to argWritables.join(","),
//                    )
//                }
//                if (ownership == Ownership.Owned) {
//                    fnWriter = fnWriter.plus(writable { rust(".to_owned()") })
//                }

                rustTemplate(
                    "#{fn}(#{args}, ${EndpointResolverGenerator.DIAGNOSTIC_COLLECTOR})",
                    "fn" to fnDefinition.usage(),
                    "args" to argWritables.join(","),
                )
                if (ownership == Ownership.Owned) {
                    rust(".to_owned()")
                }

//
//                if (fn.returnType is OptionalType) {
//
//                    rustTemplate(
//                        """if let Some(result) = #{FN:W} {
//                            result
//                        } else {
//                            return false;
//                        }
//                    """.trimMargin(),
//                        "FN" to fnWriter,
//                    )
//
//                } else {
//                    rustTemplate(
//                        """#{FN:W}""".trimMargin(),
//                        "FN" to fnWriter,
//                    )
//                }
            }
    }

    /**
     * Outer expression generator visitor that handles BDD-specific logic
     */
    private inner class OuterExprGeneratorVisitor(
        private val ownership: Ownership,
        private val idx: Int,
    ) : ExprGeneratorVisitor(ownership, 0) {
        override fun visitGetAttr(getAttr: GetAttr): Writable =
            writable {
                val expressionGenerator =
                    BddExpressionGenerator(condition, Ownership.Borrowed, context, refs, knownSomeRefs)
                if (condition.result.isPresent) {
                    // If the path contains an idx then the result is already an option
                    val pathContainsIdx = getAttr.path.toList().filter { it is GetAttr.Part.Index }.isNotEmpty()
                    if (pathContainsIdx) {
                        rustTemplate(
                            """
                            {
                                *${condition.result.get().rustName()} = #{FN:W}.map(|inner| inner.into());
                                true
                            }
                            """.trimIndent(),
                            "FN" to expressionGenerator.generateExpression(condition.function, idx),
                        )
                    } else {
                        rustTemplate(
                            """
                            {
                                *${condition.result.get().rustName()} = Some(#{FN:W}.into());
                                true
                            }
                            """.trimIndent(),
                            "FN" to expressionGenerator.generateExpression(condition.function, idx),
                        )
                    }
                } else {
                    rustTemplate(
                        """
                        #{FN:W}
                        """.trimIndent(),
                        "FN" to expressionGenerator.generateExpression(condition.function, idx),
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
                    BddExpressionGenerator(condition, Ownership.Borrowed, context, refs, knownSomeRefs)

                val fnReturnTypeOptional = condition.function.functionDefinition.returnType is OptionalType
                val fnReturnTypeIsBoolean = condition.function.functionDefinition.returnType is BooleanType

                // If the condition sets a result we do the assignment and return true to move on
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
                            "FN" to expressionGenerator.generateExpression(condition.function, idx),
                        )
                    } else if (!fnReturnTypeOptional && !fnReturnTypeIsBoolean) {
                        // Function is infallible
                        rustTemplate(
                            """
                            {
                                *${condition.result.get().rustName()} = Some(#{FN:W}.into());
                                true
                            }
                            """.trimIndent(),
                            "FN" to expressionGenerator.generateExpression(condition.function, idx),
                        )
                    } else {
                        rustTemplate(
                            """
                            {
                                *${condition.result.get().rustName()} = #{FN:W}.map(|inner| inner.into());
                                ${condition.result.get().rustName()}.is_some()
                            }
                            """.trimIndent(),
                            "FN" to expressionGenerator.generateExpression(condition.function, idx),
                        )
                    }
                } else if (!fnReturnTypeIsBoolean && fnReturnTypeOptional) {
                    rustTemplate(
                        """
                        (#{FN:W}).is_some()
                        """.trimIndent(),
                        "FN" to expressionGenerator.generateExpression(condition.function, idx),
                    )
                } else {
                    rustTemplate(
                        """
                        #{FN:W}
                        """.trimIndent(),
                        "FN" to expressionGenerator.generateExpression(condition.function, idx),
                    )
                }
            }
    }
}
