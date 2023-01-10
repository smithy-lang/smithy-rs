/*
 *  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 *  SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen

import software.amazon.smithy.rulesengine.language.syntax.expr.Expression
import software.amazon.smithy.rulesengine.language.visit.TemplateVisitor
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.util.dq

/**
 * Template Generator
 *
 * Templates can be in one of 3 possible formats:
 * 1. Single static string: `https://staticurl.com`. In this case, we return a string literal.
 * 2. Single dynamic string: `{Region}`. In this case we delegate directly to the underlying expression.
 * 3. Compound string: `https://{Region}.example.com`. In this case, we use a string builder:
 * ```rust
 * {
 *   let mut out = String::new();
 *   out.push_str("https://");
 *   out.push_str(region);
 *   out.push_str(".example.com);
 *   out
 * }
 * ```
 */
class TemplateGenerator(
    private val ownership: Ownership,
    private val exprGenerator: (Expression, Ownership) -> Writable,
) : TemplateVisitor<Writable> {
    override fun visitStaticTemplate(value: String) = writable {
        // In the case of a static template, return the literal string, eg. `"foo"`.
        rust(value.dq())
        if (ownership == Ownership.Owned) {
            rust(".to_string()")
        }
    }

    override fun visitSingleDynamicTemplate(expr: Expression): Writable {
        return exprGenerator(expr, ownership)
    }

    override fun visitStaticElement(str: String) = writable {
        when (str.length) {
            0 -> {}
            1 -> rust("out.push('$str');")
            else -> rust("out.push_str(${str.dq()});")
        }
    }

    override fun visitDynamicElement(expr: Expression) = writable {
        // we don't need to own the argument to push_str
        Attribute.AllowClippyNeedlessBorrow.render(this)
        rust("out.push_str(&#W);", exprGenerator(expr, Ownership.Borrowed))
    }

    override fun startMultipartTemplate() = writable {
        if (ownership == Ownership.Borrowed) {
            rust("&")
        }
        rust("{ let mut out = String::new();")
    }

    override fun finishMultipartTemplate() = writable {
        rust(" out }")
    }
}
