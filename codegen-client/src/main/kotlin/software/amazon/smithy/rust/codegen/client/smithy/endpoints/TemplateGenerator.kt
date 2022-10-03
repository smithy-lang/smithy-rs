package software.amazon.smithy.rust.codegen.client.smithy.endpoints

import software.amazon.smithy.rulesengine.language.syntax.expr.Expression
import software.amazon.smithy.rulesengine.language.visit.TemplateVisitor
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustInline
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.util.dq

class TemplateGenerator(
    private val ownership: EndpointResolverGenerator.Ownership,
    private val expressionGenerator: (Expression) -> Writable,
) : TemplateVisitor<Writable> {
    override fun visitStaticTemplate(s: String): Writable {
        return writable {
            rustInline(s.dq())
            if (ownership == EndpointResolverGenerator.Ownership.Owned) {
                rustInline(".to_string()")
            }
        }
    }

    override fun visitSingleDynamicTemplate(p0: Expression): Writable {
        return expressionGenerator(p0)
    }

    override fun visitStaticElement(p0: String): Writable {
        return writable {
            rustInline("out.push_str(${p0.dq()});")
        }
    }

    override fun visitDynamicElement(p0: Expression): Writable {
        return writable {
            rustInline("out.push_str(#W);", expressionGenerator(p0))
        }
    }

    override fun startMultipartTemplate(): Writable {
        return writable {
            if (ownership == EndpointResolverGenerator.Ownership.Borrowed) {
                rustInline("&")
            }
            rustInline("{ let mut out = String::new(); ")
        }
    }

    override fun finishMultipartTemplate(): Writable {
        return writable { rustInline("out }") }
    }
}
