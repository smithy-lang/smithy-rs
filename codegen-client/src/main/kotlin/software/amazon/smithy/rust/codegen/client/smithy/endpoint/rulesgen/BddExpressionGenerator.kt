/*
 *  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 *  SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen

import software.amazon.smithy.rulesengine.language.evaluation.type.ArrayType
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
    /**
     * When true, the outermost GetAttr being generated is assigning to a context
     * variable that is typed `Option<&'a str>` (see [AnnotatedRefs.AnnotatedRef.isBorrowedStr]).
     *
     * The path lowering must skip `.cloned()` on index access (to avoid allocating
     * an owned `String`), and the outer assignment wrapper must skip the
     * `.map(|inner| inner.into())` / `.into()` conversions (which would demand an
     * owned target). This flag is only true for the sub-tree of the direct
     * assignment; nested expressions within conditions retain the default behavior.
     */
    private val borrowedStrTarget: Boolean = false,
) {
    private val knownSomeRefsNames = knownSomeRefs.map { it.name }

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
        expr is Reference && refs.resolve(expr.name)?.isOptional == true

    private fun isDocumentRef(expr: Expression): Boolean =
        expr is Reference &&
            refs.resolve(expr.name)?.runtimeType == RuntimeType.document(codegenContext.runtimeConfig)

    private fun isKnownSomeRef(ref: Reference): Boolean = knownSomeRefsNames.contains(refs.resolveName(ref.name))

    /**
     * True when the reference's underlying type is a String (possibly wrapped in Optional).
     * Used to decide whether `.as_deref().unwrap_or_default()` is a legal lowering.
     * Falls back to `true` when typechecking is unavailable so template rendering at least
     * attempts the String path — most template dynamic parts are strings.
     */
    private fun isStringTypedRef(expr: Expression): Boolean {
        if (expr !is Reference) return false
        return try {
            val t = expr.type()
            val underlying = if (t is OptionalType) t.inner() else t
            underlying is StringType
        } catch (_: RuntimeException) {
            true
        }
    }

    private fun createChildGenerator(
        childOwnership: Ownership = this.ownership,
        childBorrowedStrTarget: Boolean = this.borrowedStrTarget,
    ): BddExpressionGenerator =
        BddExpressionGenerator(condition, childOwnership, context, refs, codegenContext, knownSomeRefs, childBorrowedStrTarget)

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
            // Thread BDD state through literal template expansion so that GetAttr on
            // optional assigned variables inside a template string (e.g. "{PartitionResult#attr}")
            // receives the same `if let Some(inner) = ...` unwrap as GetAttr outside a template.
            //
            // Inside templates, dynamic parts feed into `out.push_str(&...)` under forced-Borrowed
            // ownership (see TemplateGenerator.visitDynamicElement). That means any expression
            // rendered here must produce a `&str`-compatible borrow. BDD mode flattens
            // condition evaluation into match arms without the tree-based resolver's outer
            // `if let Some(x) = x` wrappers, so an optional String reference like `region`
            // (typed `&Option<String>`) needs an explicit `as_deref().unwrap_or_default()` to
            // reach `&str`. The BDD's control flow guarantees the ref is Some on any path
            // reaching this condition (prior `isSet(...)` condition edges filter None cases).
            return literal.accept(
                LiteralGenerator(ownership, context) { expr, templateOwnership ->
                    when {
                        expr is Reference &&
                            templateOwnership == Ownership.Borrowed &&
                            isOptionalRef(expr) &&
                            isStringTypedRef(expr) ->
                            writable {
                                rust("${refs.resolveName(expr.name)}.as_deref().unwrap_or_default()")
                            }
                        else ->
                            createChildGenerator(templateOwnership).generateExpression(expr)
                    }
                },
            )
        }

        override fun visitRef(ref: Reference) =
            writable {
                val name = refs.resolveName(ref.name)
                if (isKnownSomeRef(ref)) {
                    rust("""$name.expect("$name was set my a previous condition.")""")
                } else {
                    rust(name)
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
                // When assigning to an `Option<&'a str>` context var, avoid the
                // `.cloned()` that would materialize an owned String. Use
                // `.map(|s| s.as_str())` instead so the result is `Option<&'a str>`.
                val indexTail = if (borrowedStrTarget) ".map(|s| s.as_str())" else ".cloned()"
                getAttr.path.toList().forEach { part ->
                    when (part) {
                        is GetAttr.Part.Key -> rust(".${part.key().rustName()}()")
                        is GetAttr.Part.Index -> {
                            val index = part.index()
                            when {
                                index == 0 -> rust(".first()$indexTail")
                                index < 0 -> rust(".iter().nth_back(${-index - 1})$indexTail")
                                else -> rust(".get($index)$indexTail")
                            }
                        }
                    }
                }
                // Skip the to_owned/into trailer for borrowed-str targets; their
                // context slot is `&'a str`, which refuses ownership conversions.
                if (!borrowedStrTarget && ownership == Ownership.Owned && shouldAddToOwned(getAttr)) {
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
                    knownSomeRefs.add(refs.resolve(fn.name)!!)
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
            val lhsIsOptionalRef = isOptionalRef(left)
            val rhsIsOptionalRef = isOptionalRef(right)
            val (lhsInto, rhsInto) = getStringEqualityModifier(left, lhsIsOptionalRef, right, rhsIsOptionalRef)

            // When wrapping a Literal in `&mut Some(...)` for comparison against an `Option<String>`
            // reference, the wrapped expression must produce an owned `String`. Multipart template
            // strings emit `&{ let mut out = String::new(); ...; out }` under Borrowed ownership
            // (see TemplateGenerator.startMultipartTemplate), which yields `&String` — `.into()`
            // cannot coerce that into `String`, causing the outer `&mut Some(&String)` to be
            // `&mut Option<&String>` instead of `&mut Option<String>`. Under Owned ownership the
            // leading `&` is omitted, so the block returns an owned `String` and the comparison
            // types line up.
            val leftOwnership = if (rhsIsOptionalRef && left is Literal) Ownership.Owned else ownership
            val rightOwnership = if (lhsIsOptionalRef && right is Literal) Ownership.Owned else ownership
            val leftGenerator = createChildGenerator(leftOwnership)
            val rightGenerator = createChildGenerator(rightOwnership)

            conditionalBlock("&mut Some(", ")", conditional = rhsIsOptionalRef) {
                rust("(#W$lhsInto)", leftGenerator.generateExpression(left))
            }
            rust("==")
            conditionalBlock("&mut Some(", ")", conditional = lhsIsOptionalRef) {
                rust("(#W$rhsInto)", rightGenerator.generateExpression(right))
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
                (arg is StringLiteral && refs.resolve(Identifier.of(arg.value().toString()))?.isOptional == true) ||
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
                    !isOptionalArgument(arg) -> {
                        rust("#W", expressionGenerator.generateExpression(arg))
                        // Coalesce macro's autoderef specialization requires owned values to
                        // match the (Option<T>, T) impl. Params are bound as references,
                        // and string literals are &str — need conversion to owned types.
                        if (arg is Reference) {
                            rust(".clone()")
                        } else if (arg is Literal && arg.type() is StringType) {
                            rust(".to_string()")
                        }
                    }
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

                    else -> rust("#W.clone()", expressionGenerator.generateExpression(arg))
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
                val targetRef = condition.result.orElse(null)?.let { refs.resolve(it) }
                val isBorrowedTarget = targetRef?.isBorrowedStr == true
                val innerGen = createChildGenerator(childBorrowedStrTarget = isBorrowedTarget)
                val fnExpr = innerGen.generateExpression(condition.function)
                val pathContainsIdx = getAttr.path.toList().any { it is GetAttr.Part.Index }

                if (condition.result.isPresent) {
                    rust("#W", generateGetAttrAssignment(fnExpr, pathContainsIdx, isBorrowedTarget))
                } else {
                    rust("#W", generateGetAttrEvaluation(fnExpr, pathContainsIdx))
                }
            }

        private fun generateGetAttrAssignment(
            fnExpr: Writable,
            pathContainsIdx: Boolean,
            isBorrowedTarget: Boolean,
        ): Writable =
            writable {
                val resultName = refs.resolveName(condition.result.get())
                if (pathContainsIdx) {
                    // Path contains index, resulting type is Option<T>
                    if (isBorrowedTarget) {
                        // fnExpr already yields Option<&'a str> (via .map(|s| s.as_str())
                        // in generateAttrPath). Assign directly — no `.into()` wrapping.
                        rustTemplate(
                            """
                            {
                                *$resultName = #{FN:W};
                                $resultName.is_some()
                            }
                            """.trimIndent(),
                            "FN" to fnExpr,
                        )
                    } else {
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
                } else {
                    // No index in path, getting the field is infallible
                    if (isBorrowedTarget) {
                        // fnExpr yields `&'a str` directly; wrap in Some without
                        // `.into()` (which would demand an owned target).
                        rustTemplate(
                            """
                            {
                                *$resultName = Some(#{FN:W});
                                true
                            }
                            """.trimIndent(),
                            "FN" to fnExpr,
                        )
                    } else {
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
                // Use .type() for the actual resolved return type (not functionDefinition.returnType
                // which is the generic declared type and doesn't account for coalesce's type-dependent returns)
                val fnReturnType = condition.function.type()

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
                val resultName = refs.resolveName(condition.result.get())
                val isOptional = returnType is OptionalType
                val isBoolean = returnType is BooleanType
                val innerType = if (isOptional) (returnType as OptionalType).inner() else returnType
                val isArray = innerType is ArrayType

                // Conversion expression: .into() for scalars, .into_iter().map(...).collect() for arrays
                val intoExpr = if (isArray) ".into_iter().map(|s| s.to_owned()).collect()" else ".into()"

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
                                *$resultName = Some(#{FN:W}$intoExpr);
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
                                *$resultName = #{FN:W}.map(|inner| inner$intoExpr);
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
