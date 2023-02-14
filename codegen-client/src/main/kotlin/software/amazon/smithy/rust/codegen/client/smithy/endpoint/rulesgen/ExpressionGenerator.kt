/*
 *  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 *  SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen

import org.jetbrains.annotations.Contract
import software.amazon.smithy.rulesengine.language.eval.Type
import software.amazon.smithy.rulesengine.language.syntax.expr.Expression
import software.amazon.smithy.rulesengine.language.syntax.expr.Literal
import software.amazon.smithy.rulesengine.language.syntax.expr.Reference
import software.amazon.smithy.rulesengine.language.syntax.fn.FunctionDefinition
import software.amazon.smithy.rulesengine.language.syntax.fn.GetAttr
import software.amazon.smithy.rulesengine.language.visit.ExpressionVisitor
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.Context
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.EndpointResolverGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rustName
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.util.PANIC

/**
 * Root expression generator.
 */
class ExpressionGenerator(
    private val ownership: Ownership,
    private val context: Context,
) {

    @Contract(pure = true)
    fun generate(expr: Expression): Writable {
        return expr.accept(ExprGeneratorVisitor(ownership))
    }

    /**
     * Inner generator based on ExprVisitor
     */
    private inner class ExprGeneratorVisitor(
        private val ownership: Ownership,
    ) :
        ExpressionVisitor<Writable> {

        override fun visitLiteral(literal: Literal): Writable {
            return literal.accept(LiteralGenerator(ownership, context))
        }

        override fun visitRef(ref: Reference) = writable {
            if (ownership == Ownership.Owned) {
                when (ref.type()) {
                    is Type.Bool -> rust("*${ref.name.rustName()}")
                    else -> rust("${ref.name.rustName()}.to_owned()")
                }
            } else {
                rust(ref.name.rustName())
            }
        }

        override fun visitGetAttr(getAttr: GetAttr): Writable {
            val target = ExpressionGenerator(Ownership.Borrowed, context).generate(getAttr.target)
            val path = writable {
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
                if (ownership == Ownership.Owned && getAttr.type() != Type.bool()) {
                    if (getAttr.type() is Type.Option) {
                        rust(".map(|t|t.to_owned())")
                    } else {
                        rust(".to_owned()")
                    }
                }
            }
            return writable { rust("#W#W", target, path) }
        }

        override fun visitIsSet(fn: Expression) = writable {
            val expressionGenerator = ExpressionGenerator(Ownership.Borrowed, context)
            rust("#W.is_some()", expressionGenerator.generate(fn))
        }

        override fun visitNot(not: Expression) = writable {
            rust("!(#W)", ExpressionGenerator(Ownership.Borrowed, context).generate(not))
        }

        override fun visitBoolEquals(left: Expression, right: Expression) = writable {
            val expressionGenerator = ExpressionGenerator(Ownership.Owned, context)
            rust("(#W) == (#W)", expressionGenerator.generate(left), expressionGenerator.generate(right))
        }

        override fun visitStringEquals(left: Expression, right: Expression) = writable {
            val expressionGenerator = ExpressionGenerator(Ownership.Borrowed, context)
            rust("(#W) == (#W)", expressionGenerator.generate(left), expressionGenerator.generate(right))
        }

        override fun visitLibraryFunction(fn: FunctionDefinition, args: MutableList<Expression>): Writable = writable {
            val fnDefinition = context.functionRegistry.fnFor(fn.id)
                ?: PANIC(
                    "no runtime function for ${fn.id} " +
                        "(hint: if this is a custom or aws-specific runtime function, ensure the relevant standard library has been loaded " +
                        "on the classpath)",
                )
            val expressionGenerator = ExpressionGenerator(Ownership.Borrowed, context)
            val argWritables = args.map { expressionGenerator.generate(it) }
            rustTemplate(
                "#{fn}(#{args}, ${EndpointResolverGenerator.DiagnosticCollector})",
                "fn" to fnDefinition.usage(),
                "args" to argWritables.join(","),
            )
            if (ownership == Ownership.Owned) {
                rust(".to_owned()")
            }
        }
    }
}
