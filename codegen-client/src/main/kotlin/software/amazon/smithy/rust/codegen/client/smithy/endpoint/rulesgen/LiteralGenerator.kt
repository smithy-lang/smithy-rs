/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen

import software.amazon.smithy.rulesengine.language.syntax.Identifier
import software.amazon.smithy.rulesengine.language.syntax.expressions.Expression
import software.amazon.smithy.rulesengine.language.syntax.expressions.Template
import software.amazon.smithy.rulesengine.language.syntax.expressions.literal.Literal
import software.amazon.smithy.rulesengine.language.syntax.expressions.literal.LiteralVisitor
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.Context
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.dq
import java.util.stream.Stream

/**
 * Generator for literals (strings, numbers, bools, lists and objects)
 *
 * The `Document` type is used to support generating JSON-like documents
 *
 * [exprGenerator] is an optional callback used to render the dynamic parts of string templates
 * (the `{Var}` / `{Var#attr}` placeholders inside a `Template`). BDD-mode callers pass a
 * callback that delegates back to [BddExpressionGenerator] so that `getAttr` on optional
 * assigned variables correctly generates an `if let Some(inner) = ... { inner.attr() }` unwrap.
 * When omitted, the tree-based [ExpressionGenerator] is used, preserving existing behavior for
 * callers that do not need BDD-specific optional-ref tracking.
 *
 * [multipartExprGenerator] is an optional callback used **only** for dynamic elements inside
 * a multipart template (the `out.push_str(&...)` parts). When omitted, [exprGenerator] is used.
 * BDD-mode callers can use this to apply optional-unwrapping (e.g. `.as_deref().unwrap_or_default()`)
 * for `&str`-typed `push_str` arguments while keeping the single-dynamic-template case
 * (`"{ref}"`) Option-typed for the outer caller's `if let Some(param) = ...` wrappers.
 */
class LiteralGenerator(
    private val ownership: Ownership,
    private val context: Context,
    private val exprGenerator: ((Expression, Ownership) -> Writable)? = null,
    private val multipartExprGenerator: ((Expression, Ownership) -> Writable)? = null,
) :
    LiteralVisitor<Writable> {
    private val runtimeConfig = context.runtimeConfig
    private val codegenScope =
        arrayOf(
            "Document" to RuntimeType.document(runtimeConfig),
            "DocumentObject" to RuntimeType.documentObject(runtimeConfig),
        )

    private fun renderTemplateExpression(
        expr: Expression,
        exprOwnership: Ownership,
    ): Writable =
        exprGenerator?.invoke(expr, exprOwnership)
            ?: ExpressionGenerator(exprOwnership, context).generate(expr)

    private fun renderMultipartExpression(
        expr: Expression,
        exprOwnership: Ownership,
    ): Writable =
        multipartExprGenerator?.invoke(expr, exprOwnership)
            ?: renderTemplateExpression(expr, exprOwnership)

    override fun visitBoolean(b: Boolean) =
        writable {
            rust(b.toString())
        }

    override fun visitString(value: Template) =
        writable {
            val parts: Stream<Writable> =
                value.accept(
                    TemplateGenerator(
                        ownership,
                        { expr, templateOwnership ->
                            renderTemplateExpression(expr, templateOwnership)
                        },
                        { expr, templateOwnership ->
                            renderMultipartExpression(expr, templateOwnership)
                        },
                    ),
                )
            parts.forEach { part -> part(this) }
        }

    override fun visitRecord(members: MutableMap<Identifier, Literal>) =
        writable {
            rustBlock("") {
                rustTemplate(
                    "let mut out = #{DocumentObject}::with_capacity(${members.size});",
                    *codegenScope,
                )
                members.keys.sortedBy { it.toString() }.map { k -> k to members[k]!! }.forEach { (identifier, literal) ->
                    rust(
                        "out.insert(${identifier.toString().dq()}.to_string(), #W.into());",
                        // When writing into the hashmap, it always needs to be an owned type
                        ExpressionGenerator(Ownership.Owned, context).generate(literal),
                    )
                }
                rustTemplate("out")
            }
        }

    override fun visitTuple(members: MutableList<Literal>) =
        writable {
            rustTemplate(
                "vec![#{inner:W}]", *codegenScope,
                "inner" to
                    writable {
                        members.forEach { literal ->
                            rustTemplate(
                                "#{Document}::from(#{literal:W}),",
                                *codegenScope,
                                "literal" to
                                    ExpressionGenerator(
                                        Ownership.Owned,
                                        context,
                                    ).generate(literal),
                            )
                        }
                    },
            )
        }

    override fun visitInteger(value: Int) =
        writable {
            rust("$value")
        }
}
