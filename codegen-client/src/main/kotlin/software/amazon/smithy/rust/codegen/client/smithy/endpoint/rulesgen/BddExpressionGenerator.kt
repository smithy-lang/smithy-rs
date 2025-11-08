/*
 *  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 *  SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen

import software.amazon.smithy.rulesengine.language.evaluation.type.BooleanType
import software.amazon.smithy.rulesengine.language.evaluation.type.OptionalType
import software.amazon.smithy.rulesengine.language.evaluation.type.StringType
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
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rustName
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.util.PANIC
import java.lang.RuntimeException

/**
 * Root expression generator.
 */
class BddExpressionGenerator(
    private val ownership: Ownership,
    private val context: Context,
    private val refs: List<AnnotatedRef>,
) {
    private val optionalRefNames = refs.filter { it.isOptional }.map { it.name }

    fun generateCondition(
        condition: Condition,
        idx: Int,
    ): Writable {
        return generateExpression(condition.function, idx)
    }

    private fun generateExpression(
        expr: Expression,
        idx: Int,
    ): Writable {
        return expr.accept(
            {
                println("LNJ COND IDX: $idx")
                ExprGeneratorVisitor(ownership, idx)
            }(),
        )
    }

    /**
     * Inner generator based on ExprVisitor
     */
    private inner class ExprGeneratorVisitor(
        private val ownership: Ownership,
        private val idx: Int,
    ) :
        ExpressionVisitor<Writable> {
        override fun visitLiteral(literal: Literal): Writable {
            return literal.accept(LiteralGenerator(ownership, context))
        }

        override fun visitRef(ref: Reference) =
            writable {
                if (ownership == Ownership.Owned) {
                    try {
                        when (ref.type()) {
                            is BooleanType -> rust("*${ref.name.rustName()}")
                            else -> rust("${ref.name.rustName()}.to_owned()")
                        }
                    } catch (_: RuntimeException) {
                        // Typechecking was never invoked - default to .to_owned()
                        rust("${ref.name.rustName()}.to_owned()")
                    }
                } else {
                    try {
                        when (ref.type()) {
                            // This ensures we obtain a `&str`, regardless of whether `ref.name.rustName()` returns a `String` or a `&str`.
                            // Typically, we don't know which type will be returned due to code generation.
                            is StringType -> rust("${ref.name.rustName()}.as_ref() as &str")
                            else -> rust(ref.name.rustName())
                        }
                    } catch (_: RuntimeException) {
                        // Because Typechecking was never invoked upon calling `.type()` on Reference for an expression
                        // like "{ref}: rust". See `generateLiterals2` in ExprGeneratorTest.
                        rust(ref.name.rustName())
                    }
                }
            }

        override fun visitGetAttr(getAttr: GetAttr): Writable {
            val target =
                BddExpressionGenerator(Ownership.Borrowed, context, refs).generateExpression(getAttr.target, idx)
            val targetIsOptionalRef =
                getAttr.target is Reference && optionalRefNames.contains((getAttr.target as Reference).name.rustName())
            val targetRustName =
                if (targetIsOptionalRef) {
                    (getAttr.target as Reference).name.rustName()
                } else {
                    "doesntmatter"
                }
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
                writable { rust("""#W.expect("$targetRustName should already be set")#W""", target, path) }
            } else {
                writable { rust("#W#W", target, path) }
            }
        }

        override fun visitIsSet(fn: Expression) =
            writable {
                val expressionGenerator = BddExpressionGenerator(Ownership.Borrowed, context, refs)
                rust("#W.is_some()", expressionGenerator.generateExpression(fn, idx))
            }

        override fun visitNot(not: Expression) =
            writable {
                rust("!(#W)", BddExpressionGenerator(Ownership.Borrowed, context, refs).generateExpression(not, idx))
            }

        override fun visitBoolEquals(
            left: Expression,
            right: Expression,
        ) = writable {
            val expressionGenerator = BddExpressionGenerator(Ownership.Owned, context, refs)
            rust(
                "(#W) == (#W)",
                expressionGenerator.generateExpression(left, idx),
                expressionGenerator.generateExpression(right, idx),
            )
        }

        override fun visitStringEquals(
            left: Expression,
            right: Expression,
        ) = writable {
            val expressionGenerator = BddExpressionGenerator(Ownership.Borrowed, context, refs)
            val lhsIsOptionalRef = (left is Reference && optionalRefNames.contains(left.name.rustName()))
            val rhsIsOptionalRef = (right is Reference && optionalRefNames.contains(right.name.rustName()))

            // TODO(BDD) this logic would probably look nicer with a writer util like conditionalBlock
            // but it is conditional parens and you can throw a Some (or other enum variant) in front
            if (rhsIsOptionalRef) {
                rust("Some(")
            }
            rust(
                "(#W)",
                expressionGenerator.generateExpression(left, idx),
            )
            if (rhsIsOptionalRef) {
                rust(")")
            }

            rust("==")

            if (lhsIsOptionalRef) {
                rust("Some(")
            }
            rust(
                "(#W)",
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
                    val expressionGenerator = BddExpressionGenerator(Ownership.Borrowed, context, refs)
                    val argWritables = args.map { expressionGenerator.generateExpression(it, idx) }

                    // Generate: arg1.or(arg2).or(arg3)...
                    if (argWritables.isEmpty()) {
                        rust("None")
                    } else if (argWritables.size == 1) {
                        rust("#W", argWritables[0])
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
                val expressionGenerator = BddExpressionGenerator(Ownership.Borrowed, context, refs)
                val argWritables =
                    args.map { arg ->
                        println("LNJ ARG: $arg")
                        println("ARG IS REF ${arg is Reference}")
                        if (arg is Reference) {
                            println("ARG RUSTNAME: ${arg.name.rustName()}")
                            println("ARG IN OPTIONALREFS: ${optionalRefNames.contains(arg.name.rustName())}")
                        }
//                    println("LNJ OPTIONALREFS: $optionalRefs")
                        try {
//                        val argType = arg.type()
                            if ((arg is Reference && optionalRefNames.contains(arg.name.rustName())) ||
                                (arg is LibraryFunction && arg.functionDefinition.returnType is OptionalType)
                            ) {
                                println("LNJ ARG IS OPTIONAL")
                                writable {
                                    rust(
                                        """
                                        if let Some(param) = #W{
                                            param
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
                        } catch (_: RuntimeException) {
                            // Typechecking not available - default to .to_owned()
                            expressionGenerator.generateExpression(arg, idx)
                        }
                    }

                rustTemplate(
                    "#{fn}(#{args}, ${EndpointResolverGenerator.DIAGNOSTIC_COLLECTOR})",
                    "fn" to fnDefinition.usage(),
                    "args" to argWritables.join(","),
                )
                if (ownership == Ownership.Owned) {
                    rust(".to_owned()")
                }
            }
    }
}
