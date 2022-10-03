package software.amazon.smithy.rust.codegen.client.smithy.endpoints

import org.jetbrains.annotations.Contract
import software.amazon.smithy.rulesengine.language.Endpoint
import software.amazon.smithy.rulesengine.language.EndpointRuleSet
import software.amazon.smithy.rulesengine.language.eval.Type
import software.amazon.smithy.rulesengine.language.stdlib.BooleanEquals
import software.amazon.smithy.rulesengine.language.stdlib.StringEquals
import software.amazon.smithy.rulesengine.language.syntax.expr.Expression
import software.amazon.smithy.rulesengine.language.syntax.expr.Literal
import software.amazon.smithy.rulesengine.language.syntax.expr.Reference
import software.amazon.smithy.rulesengine.language.syntax.expr.Template
import software.amazon.smithy.rulesengine.language.syntax.fn.Function
import software.amazon.smithy.rulesengine.language.syntax.fn.GetAttr
import software.amazon.smithy.rulesengine.language.syntax.fn.IsSet
import software.amazon.smithy.rulesengine.language.syntax.fn.Not
import software.amazon.smithy.rulesengine.language.syntax.rule.Condition
import software.amazon.smithy.rulesengine.language.syntax.rule.EndpointRule
import software.amazon.smithy.rulesengine.language.syntax.rule.ErrorRule
import software.amazon.smithy.rulesengine.language.syntax.rule.Rule
import software.amazon.smithy.rulesengine.language.syntax.rule.TreeRule
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.asType
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustInline
import software.amazon.smithy.rust.codegen.core.rustlang.rustInlineTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTuple
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.toRustName

class EndpointResolverGenerator(
    codegenContext: ClientCodegenContext,
    private val endpointRuleset: EndpointRuleSet,
    endpointParams: RuntimeType,
    endpointError: RuntimeType,
    endpointLib: InlineDependency,
) {
    private val scope = arrayOf(
        "Endpoint" to CargoDependency.SmithyHttp(codegenContext.runtimeConfig).asType().member("endpoint::Endpoint"),
        "endpoint_lib" to RuntimeType.forInlineDependency(endpointLib),
        "Params" to endpointParams,
        "Error" to endpointError,
    )

    private var nameIdx = 0

    private fun nameFor(expression: Expression): String {
        nameIdx += 1
        return when (expression) {
            is Reference -> expression.name.rustName()
            else -> "var_$nameIdx"
        }
    }

    sealed class Ownership {
        object Borrowed : Ownership()
        object Owned : Ownership()
    }

    data class Context(val valueNumbering: Map<Expression, String>) {
        fun withValue(expression: Expression, name: String): Context {
            return this.copy(valueNumbering = valueNumbering.plus(expression to name))
        }

        companion object {
            fun empty() = Context(HashMap())
        }
    }

    fun generateResolver(crate: RustCrate) {
        crate.withNonRootModule("crate::endpoints::resolver") { writer ->
            writer.rustBlockTemplate(
                """
                pub(crate) fn resolve_endpoint(
                    params: #{Params},
                ) -> Result<#{Endpoint}, #{Error}>
                """,
                *scope,
            ) {
                // Need to run this, or we can't check the type of any `Expr`s later
                endpointRuleset.typecheck()
                endpointRuleset.parameters.toList().forEach {
                    rust("let ${it.name.rustName()} = &params.${it.name.rustName()};")
                }
                rustTemplate("#{rules:W}", "rules" to treeRule(listOf(), endpointRuleset.rules, Context.empty()))
            }
        }
    }


    private fun treeRule(conditions: List<Condition>, rules: List<Rule>, ctx: Context): Writable {
        return writable {
            generateCondition(
                conditions,
                { ctx ->
                    {
                        rules.forEach { rule ->
                            generateCondition(rule.conditions, { ctx -> ruleBody(rule, ctx) }, ctx)(this)
                        }
                        if (rules.last().conditions.isNotEmpty()) {
                            rust("""return Err(format!("No rules matched. Parameters: {:?}", params).into())""")
                        }
                    }
                },
                ctx,
            )(this)
        }
    }

    private fun ruleBody(rule: Rule, ctx: Context): Writable = writable {
        when (rule) {
            is EndpointRule -> rust("return Ok(#W);", generateEndpoint(rule.endpoint, ctx))
            is ErrorRule -> rust("return Err(#W.into());", generateExpr(rule.error, ctx, Ownership.Owned))
            is TreeRule -> {
                rule.rules.forEach { subRule ->
                    generateCondition(
                        subRule.conditions,
                        { ctx -> ruleBody(subRule, ctx) },
                        ctx,
                    )(this)
                }
            }
        }
    }

    private fun generateEndpoint(endpoint: Endpoint, ctx: Context) = writable {
        rustTemplate(
            """#{Endpoint}::mutable(#{uri:W}.parse().expect("invalid URI"))""",
            "uri" to generateExpr(endpoint.url, ctx, Ownership.Owned),
            *scope,
        )
    }

    @Contract(pure = true)
    fun generateCondition(condition: List<Condition>, body: (Context) -> Writable, ctx: Context): Writable = writable {
        if (condition.isEmpty()) {
            body(ctx)(this)
        } else {
            val (head, tail) = condition.first() to condition.drop(1)
            val condName = head.result.orNull()?.rustName() ?: nameFor(head.toFn())
            val fn = head.toFn()
            when {
                fn.type() is Type.Option -> rustTemplate(
                    "if let Some($condName) = #{target:W} { #{next:W} }",
                    "target" to generateExpr(fn, ctx, Ownership.Borrowed),
                    "next" to generateCondition(tail, body, ctx.withValue(fn, condName)),
                )

                else -> {
                    check(head.result.isEmpty)
                    rustTemplate(
                        """if #{truthy:W} { #{next:W} }""",
                        "truthy" to truthy(generateExpr(head.fn, ctx, Ownership.Owned), head.fn.type()),
                        "next" to generateCondition(tail, body, ctx),
                    )
                }
            }
        }
    }

    private fun truthy(name: Writable, type: Type): Writable = writable {
        when (type) {
            is Type.Bool -> name(this)
            is Type.String -> rustInline("#W.len() > 0", name)
            else -> error("invalid: $type")
        }
    }

    private fun generateExpr(expr: Expression, ctx: Context, ownership: Ownership): Writable = writable {
        if (ctx.valueNumbering.contains(expr)) {
            if (expr.type() is Type.Bool) {
                rust("*")
            }
            rust(ctx.valueNumbering[expr]!!)
        } else {
            when (expr) {
                is IsSet -> {
                    rustInline("#W.is_some()", generateExpr(expr.target, ctx, Ownership.Borrowed))
                }

                is Not -> {
                    rustInline("!#W", generateExpr(expr.target, ctx, Ownership.Owned))
                }

                is GetAttr -> {
                    getAttr(expr, ctx)
                }

                is StringEquals -> {
                    val (left, right) = expr.arguments
                    rustInline(
                        "#W == #W",
                        generateExpr(left, ctx, Ownership.Borrowed),
                        generateExpr(right, ctx, Ownership.Borrowed),
                    )
                }

                is BooleanEquals -> {
                    val (left, right) = expr.arguments
                    rustInline(
                        "#W == #W",
                        generateExpr(left, ctx, Ownership.Owned),
                        generateExpr(right, ctx, Ownership.Owned),
                    )
                }

                is Function -> {
                    val name = expr.name.toRustName()
                    rustInlineTemplate("#{endpoint_lib}::$name", *scope)
                    rustTuple() {
                        val args = expr.arguments.iterator()
                        while (args.hasNext()) {
                            generateExpr(args.next(), ctx, ownership)
                            if (args.hasNext()) {
                                rustInline(", ")
                            }
                        }
                    }
                }

                is Reference -> {
                    rustInline("${if (ownership == Ownership.Owned) "*" else ""}${expr.name.rustName()}")
                }

                is Literal -> {
                    when (expr.type()) {
                        is Type.Bool -> rustInline("$expr")
                        is Type.String -> {
                            val template = Template.fromString(expr.toString())
                            // Check if template is really just a string literal
                            // If it has a single part, then it's a string
                            // If it has multiple parts, then it's a template
                            if (template.parts.size == 1) {
                                // Need to escape the string in case it contains valid Java formatting directives
                                rustInline(escape(expr.toString()))
                            } else {
                                generateTemplate(template, ctx, ownership)
                            }

                            // TODO Service teams should use refs when they want to refer to a single value
                            //    Therefore, counting the parts should be a valid strategy, but is it really?
                        }
                    }
                }

                else -> {
                    println("unhandled expr type: ${expr.type()}")
                    println("unhandled expr template: ${expr.template()}")
                    TODO("unhandled expr: $expr")
                }
            }
        }
    }

    private fun RustWriter.getAttr(getAttr: GetAttr, ctx: Context) {
        generateExpr(getAttr.target, ctx, Ownership.Borrowed)(this)
        getAttr.path.toList().forEach { part ->
            when (part) {
                is GetAttr.Part.Key -> rustInline(".${part.key().rustName()}")
                is GetAttr.Part.Index -> rustInline(".get(${part.index()}).cloned()") // we end up with Option<&&T>, we need to get to Option<&T>
            }
        }
    }

    private fun RustWriter.generateTemplate(template: Template, ctx: Context, ownership: Ownership) {
        val parts =
            template.accept(
                TemplateGenerator(ownership) { expression ->
                    generateExpr(
                        expression,
                        ctx,
                        Ownership.Borrowed,
                    )
                },
            )
        parts.forEach { it(this) }
    }
}

private fun Condition.toFn(): Expression {
    return when (val fn = this.fn) {
        is IsSet -> fn.target
        else -> fn
    }
}

