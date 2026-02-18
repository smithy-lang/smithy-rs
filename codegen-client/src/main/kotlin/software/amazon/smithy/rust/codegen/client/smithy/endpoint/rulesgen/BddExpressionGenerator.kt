/*
 *  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 *  SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen

import software.amazon.smithy.rulesengine.language.evaluation.type.BooleanType
import software.amazon.smithy.rulesengine.language.evaluation.type.OptionalType
import software.amazon.smithy.rulesengine.language.evaluation.type.StringType
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
    /**
     * Tracks references that are known to be Some at this point in evaluation.
     * This allows us to unwrap them safely without additional checks.
     */
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

    // ============================================================================
    // Reference Type Helpers
    // ============================================================================

    private fun isOptionalRef(expr: Expression): Boolean =
        expr is Reference && optionalRefNames.contains(expr.name.rustName())

    private fun isDocumentRef(expr: Expression): Boolean =
        expr is Reference && documentRefNames.contains(expr.name.rustName())

    private fun isKnownSomeRef(ref: Reference): Boolean = knownSomeRefsNames.contains(ref.name.rustName())

    private fun createChildGenerator(): BddExpressionGenerator =
        BddExpressionGenerator(condition, ownership, context, refs, codegenContext, knownSomeRefs)

    // ============================================================================
    // Expression Generation - Inner Visitor
    // ============================================================================

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
                if (isKnownSomeRef(ref)) {
                    rust("""${ref.name.rustName()}.expect("${ref.name.rustName()} was set my a previous condition.")""")
                } else {
                    rust(ref.name.rustName())
                }
            }

        override fun visitGetAttr(getAttr: GetAttr): Writable {
            val target = createChildGenerator().generateExpression(getAttr.target)
            val path = generateAttrPath(getAttr)

            return if (isOptionalRef(getAttr.target)) {
                wrapInOptionalCheck(target, path)
            } else {
                writable { rust("#W#W", target, path) }
            }
        }

        private fun generateAttrPath(getAttr: GetAttr): Writable =
            writable {
                getAttr.path.toList().forEach { part ->
                    when (part) {
                        is GetAttr.Part.Key -> rust(".${part.key().rustName()}()")
                        is GetAttr.Part.Index -> {
                            if (part.index() == 0) {
                                // In this case, `.first()` is more idiomatic and `.get(0)` triggers lint warnings
                                rust(".first().cloned()")
                            } else {
                                rust(".get(${part.index()}).cloned()")
                            }
                        }
                    }
                }
                if (ownership == Ownership.Owned && shouldAddToOwned(getAttr)) {
                    rust(if (getAttr.type() is OptionalType) ".map(|t|t.to_owned())" else ".to_owned()")
                }
            }

        private fun shouldAddToOwned(getAttr: GetAttr): Boolean {
            return try {
                getAttr.type() != Type.booleanType()
            } catch (_: RuntimeException) {
                true // Default to .to_owned() when typechecking unavailable
            }
        }

        private fun wrapInOptionalCheck(
            target: Writable,
            path: Writable,
        ): Writable =
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

        override fun visitIsSet(fn: Expression) =
            writable {
                val expressionGenerator = createChildGenerator()
                if (fn is Reference) {
                    // All references are in refs so safe to assert non-null
                    knownSomeRefs.add(refs.get(fn.name.rustName())!!)
                }
                rust("#W.is_some()", expressionGenerator.generateExpression(fn))
            }

        override fun visitNot(not: Expression) =
            writable {
                rust("!(#W)", createChildGenerator().generateExpression(not))
            }

        override fun visitBoolEquals(
            left: Expression,
            right: Expression,
        ) = writable {
            val expressionGenerator = createChildGenerator()
            val lhsIsOptionalRef = isOptionalRef(left)
            val rhsIsOptionalRef = isOptionalRef(right)
            val (lhsRef, rhsRef) = getBoolEqualityModifier(left, right)

            conditionalBlock("&mut Some(", ")", conditional = rhsIsOptionalRef) {
                rust("($lhsRef#W)", expressionGenerator.generateExpression(left))
            }
            rust("==")
            conditionalBlock("&mut Some(", ")", conditional = lhsIsOptionalRef) {
                rust("($rhsRef#W)", expressionGenerator.generateExpression(right))
            }
        }

        override fun visitStringEquals(
            left: Expression,
            right: Expression,
        ) = writable {
            val expressionGenerator = createChildGenerator()
            val lhsIsOptionalRef = isOptionalRef(left)
            val rhsIsOptionalRef = isOptionalRef(right)
            val (lhsInto, rhsInto) = getStringEqualityModifier(left, lhsIsOptionalRef, right, rhsIsOptionalRef)

            conditionalBlock("&mut Some(", ")", conditional = rhsIsOptionalRef) {
                rust("(#W$lhsInto)", expressionGenerator.generateExpression(left))
            }
            rust("==")
            conditionalBlock("&mut Some(", ")", conditional = lhsIsOptionalRef) {
                rust("(#W$rhsInto)", expressionGenerator.generateExpression(right))
            }
        }

        private fun getBoolEqualityModifier(
            left: Expression,
            right: Expression,
        ): Pair<String, String> {
            val lhsIsRef = left is Reference
            val rhsIsRef = right is Reference
            val lhsIsOptionalRef = isOptionalRef(left)
            val rhsIsOptionalRef = isOptionalRef(right)

            val rhsRef = if (lhsIsRef && !lhsIsOptionalRef && right is Literal) "&" else ""
            val lhsRef = if (rhsIsRef && !rhsIsOptionalRef && left is Literal) "&" else ""

            return Pair(lhsRef, rhsRef)
        }

        private fun getStringEqualityModifier(
            left: Expression,
            lhsIsOptionalRef: Boolean,
            right: Expression,
            rhsIsOptionalRef: Boolean,
        ): Pair<String, String> {
            // References are always stored as String, but functions (GetAttr, coalesce, etc) often produce
            // &str, so we .into() those result to match the reference.
            val rhsInto =
                if (left is Reference && lhsIsOptionalRef && (right is Literal || right is GetAttr)) ".into()" else ""
            val lhsInto =
                if (right is Reference && rhsIsOptionalRef && (left is Literal || left is GetAttr)) ".into()" else ""

            return Pair(lhsInto, rhsInto)
        }

        private fun isOptionalArgument(arg: Expression): Boolean {
            return isOptionalRef(arg) ||
                (arg is StringLiteral && optionalRefNames.contains(Identifier.of(arg.value().toString()).rustName())) ||
                (arg is LibraryFunction && arg.functionDefinition.returnType is OptionalType)
        }

        /**
         * Wraps function arguments based on the function type.
         * Coalesce and ite functions need special handling for optional arguments.
         */
        private fun wrapArgument(
            arg: Expression,
            fn: FunctionDefinition,
        ): Writable {
            val expressionGenerator = createChildGenerator()
            return when (fn.id) {
                "coalesce" -> wrapCoalesceArg(arg, expressionGenerator)
                "ite" -> wrapIteArg(arg, expressionGenerator)
                else -> wrapDefaultArg(arg, expressionGenerator)
            }
        }

        private fun wrapCoalesceArg(
            arg: Expression,
            expressionGenerator: BddExpressionGenerator,
        ): Writable =
            writable {
                when {
                    !isOptionalArgument(arg) -> rust("#W", expressionGenerator.generateExpression(arg))
                    arg is LibraryFunction && arg.functionDefinition.returnType is OptionalType ->
                        rust(
                            """
                            if let Some(inner) = #W{
                                inner
                            } else {
                                return false
                            }
                            """.trimMargin(),
                            expressionGenerator.generateExpression(arg),
                        )

                    else -> rust("*#W", expressionGenerator.generateExpression(arg))
                }
            }

        private fun wrapIteArg(
            arg: Expression,
            expressionGenerator: BddExpressionGenerator,
        ): Writable =
            writable {
                when {
                    arg is StringLiteral ->
                        rust("#W.to_string()", expressionGenerator.generateExpression(arg))

                    arg is GetAttr && arg.type() is StringType ->
                        rust("#W.to_string()", expressionGenerator.generateExpression(arg))

                    !isOptionalArgument(arg) ->
                        rust("#W", expressionGenerator.generateExpression(arg))

                    isOptionalRef(arg) ->
                        rust(
                            "#W.clone().expect(\"Reference already confirmed Some\")",
                            expressionGenerator.generateExpression(arg),
                        )

                    else ->
                        rust("*#W", expressionGenerator.generateExpression(arg))
                }
            }

        private fun wrapDefaultArg(
            arg: Expression,
            expressionGenerator: BddExpressionGenerator,
        ): Writable {
            if (!isOptionalArgument(arg)) {
                return expressionGenerator.generateExpression(arg)
            }
            return writable {
                val param =
                    if (isDocumentRef(arg)) {
                        """&param.as_string().expect("Document was string type.")"""
                    } else {
                        "param"
                    }

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

                val argWritables = args.map { wrapArgument(it, fn) }

                val template =
                    if (fn.id == "coalesce" || fn.id == "ite") {
                        "#{fn}(#{args})"
                    } else {
                        "#{fn}(#{args}, ${EndpointResolverGenerator.DIAGNOSTIC_COLLECTOR})"
                    }

                rustTemplate(template, "fn" to fnDefinition.usage(), "args" to argWritables.join(","))

                if (ownership == Ownership.Owned) {
                    rust(".to_owned()")
                }
            }
    }

    // ============================================================================
    // Expression Generation - Outer Visitor (handles assignments)
    // ============================================================================

    /**
     * Outer expression generator visitor that handles cases that only happen in the
     * outermost evaluation, like assignment to condition context.
     */
    private inner class OuterExprGeneratorVisitor(
        private val ownership: Ownership,
    ) : ExprGeneratorVisitor(ownership) {
        /**
         * Handles getAttr when it appears as the outermost condition expression.
         * Special case: assigns the result to the condition context if present.
         */
        override fun visitGetAttr(getAttr: GetAttr): Writable =
            writable {
                val innerGen = createChildGenerator()
                val fnExpr = innerGen.generateExpression(condition.function)
                val pathContainsIdx = getAttr.path.toList().any { it is GetAttr.Part.Index }

                if (condition.result.isPresent) {
                    rust("#W", generateGetAttrAssignment(fnExpr, pathContainsIdx))
                } else {
                    rust("#W", generateGetAttrEvaluation(fnExpr, pathContainsIdx))
                }
            }

        private fun generateGetAttrAssignment(
            fnExpr: Writable,
            pathContainsIdx: Boolean,
        ): Writable =
            writable {
                val resultName = condition.result.get().rustName()
                if (pathContainsIdx) {
                    // Path contains index, resulting type is Option<T>
                    rustTemplate(
                        """
                        {
                            *$resultName = #{FN:W}.map(|inner| inner.into());
                            $resultName.is_some()
                        }
                        """.trimIndent(),
                        "FN" to fnExpr,
                    )
                } else {
                    // No index in path, getting the field is infallible
                    rustTemplate(
                        """
                        {
                            *$resultName = Some(#{FN:W}.into());
                            true
                        }
                        """.trimIndent(),
                        "FN" to fnExpr,
                    )
                }
            }

        private fun generateGetAttrEvaluation(
            fnExpr: Writable,
            pathContainsIdx: Boolean,
        ): Writable =
            writable {
                rustTemplate(
                    """
                    #{FN:W}
                    ${if (pathContainsIdx) ".is_some()" else ""}
                    """.trimIndent(),
                    "FN" to fnExpr,
                )
            }

        override fun visitLibraryFunction(
            fn: FunctionDefinition,
            args: MutableList<Expression>,
        ): Writable =
            writable {
                val innerGen = createChildGenerator()
                val fnExpr = innerGen.generateExpression(condition.function)
                val fnReturnType = condition.function.functionDefinition.returnType

                if (condition.result.isPresent) {
                    rust("#W", generateLibraryFunctionAssignment(fnExpr, fnReturnType))
                } else {
                    rust("#W", generateLibraryFunctionEvaluation(fnExpr, fnReturnType))
                }
            }

        private fun generateLibraryFunctionAssignment(
            fnExpr: Writable,
            returnType: Type,
        ): Writable =
            writable {
                val resultName = condition.result.get().rustName()
                val isOptional = returnType is OptionalType
                val isBoolean = returnType is BooleanType

                when {
                    // Non-optional boolean: assign and return the value
                    !isOptional && isBoolean ->
                        rustTemplate(
                            """
                            {
                                let temp = #{FN:W}.into();
                                *$resultName = Some(temp);
                                temp
                            }
                            """.trimIndent(),
                            "FN" to fnExpr,
                        )
                    // Non-optional non-boolean: infallible, always return true
                    !isOptional && !isBoolean ->
                        rustTemplate(
                            """
                            {
                                *$resultName = Some(#{FN:W}.into());
                                true
                            }
                            """.trimIndent(),
                            "FN" to fnExpr,
                        )
                    // Optional: assign and check if Some
                    else ->
                        rustTemplate(
                            """
                            {
                                *$resultName = #{FN:W}.map(|inner| inner.into());
                                $resultName.is_some()
                            }
                            """.trimIndent(),
                            "FN" to fnExpr,
                        )
                }
            }

        private fun generateLibraryFunctionEvaluation(
            fnExpr: Writable,
            returnType: Type,
        ): Writable =
            writable {
                val isOptional = returnType is OptionalType
                val isBoolean = returnType is BooleanType

                // For optional non-boolean types, Some(_) is true and None is false
                if (!isBoolean && isOptional) {
                    rustTemplate("(#{FN:W}).is_some()", "FN" to fnExpr)
                } else {
                    rustTemplate("#{FN:W}", "FN" to fnExpr)
                }
            }
    }
}
