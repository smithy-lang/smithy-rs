/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen

import software.amazon.smithy.rulesengine.language.syntax.Identifier
import software.amazon.smithy.rulesengine.language.syntax.expr.Literal
import software.amazon.smithy.rulesengine.language.syntax.expr.Template
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
 */
class LiteralGenerator(private val ownership: Ownership, private val context: Context) :
    Literal.Vistor<Writable> {
    private val runtimeConfig = context.runtimeConfig
    private val codegenScope = arrayOf(
        "Document" to RuntimeType.document(runtimeConfig),
        "HashMap" to RuntimeType.HashMap,
    )
    override fun visitBool(b: Boolean) = writable {
        rust(b.toString())
    }

    override fun visitString(value: Template) = writable {
        val parts: Stream<Writable> = value.accept(
            TemplateGenerator(ownership) { expr, ownership ->
                ExpressionGenerator(ownership, context).generate(expr)
            },
        )
        parts.forEach { part -> part(this) }
    }

    override fun visitRecord(members: MutableMap<Identifier, Literal>) = writable {
        rustBlock("") {
            rustTemplate(
                "let mut out = #{HashMap}::<String, #{Document}>::new();",
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

    override fun visitTuple(members: MutableList<Literal>) = writable {
        rustTemplate(
            "vec![#{inner:W}]", *codegenScope,
            "inner" to writable {
                members.forEach { literal ->
                    rustTemplate(
                        "#{Document}::from(#{literal:W}),",
                        *codegenScope,
                        "literal" to ExpressionGenerator(
                            Ownership.Owned,
                            context,
                        ).generate(literal),
                    )
                }
            },
        )
    }

    override fun visitInteger(value: Int) = writable {
        rust("$value")
    }
}
