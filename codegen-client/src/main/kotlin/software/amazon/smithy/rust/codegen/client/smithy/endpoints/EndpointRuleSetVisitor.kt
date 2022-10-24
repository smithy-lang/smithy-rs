/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoints

import software.amazon.smithy.rulesengine.language.Endpoint
import software.amazon.smithy.rulesengine.language.EndpointRuleSet
import software.amazon.smithy.rulesengine.language.eval.Type
import software.amazon.smithy.rulesengine.language.syntax.expr.Expression
import software.amazon.smithy.rulesengine.language.syntax.expr.Literal
import software.amazon.smithy.rulesengine.language.syntax.expr.Reference
import software.amazon.smithy.rulesengine.language.syntax.expr.Template
import software.amazon.smithy.rulesengine.language.syntax.fn.FunctionDefinition
import software.amazon.smithy.rulesengine.language.syntax.fn.GetAttr
import software.amazon.smithy.rulesengine.language.syntax.fn.GetAttr.Part
import software.amazon.smithy.rulesengine.language.syntax.fn.IsSet
import software.amazon.smithy.rulesengine.language.syntax.rule.Rule
import software.amazon.smithy.rulesengine.language.visit.DefaultVisitor
import software.amazon.smithy.rulesengine.language.visit.TemplateVisitor
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.asType
import software.amazon.smithy.rust.codegen.core.rustlang.flatten
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustInline
import software.amazon.smithy.rust.codegen.core.rustlang.rustInlineTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTuple
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.letIf

internal class EndpointRuleSetVisitor(
    private val runtimeConfig: RuntimeConfig,
    private val ruleset: EndpointRuleSet,
) : DefaultVisitor<Writable>(), TemplateVisitor<Writable> {
    private val endpointLib = RuntimeType.endpointLib(runtimeConfig)

    fun visitRuleset(): Writable {
        return visitTreeRule(ruleset.rules)
    }

    private fun handleRule(rule: Rule): Writable {
        return writable {
            if (rule.conditions.isEmpty()) {
                rust("#W", rule.accept(this@EndpointRuleSetVisitor))
            } else {
                for (condition in rule.conditions) {
                    if (condition.result.isPresent) {
                        // Conditions that we treat as assignments
                        rustInline("if let Some(${condition.toRustName()}) = #W", condition.fn.accept(this@EndpointRuleSetVisitor))
                    } else if (condition.fn.type() is Type.Option) {
                        // Conditions that we treat as options
                        rustInline("if #W.is_some()", condition.fn.accept(this@EndpointRuleSetVisitor))
                    } else {
                        // Conditions that we treat as booleans
                        rustInline("if #W", condition.fn.accept(this@EndpointRuleSetVisitor))
                    }
                    openBlock(" {")
                }

                rust("#W", rule.accept(this@EndpointRuleSetVisitor))

                for (condition in rule.conditions) {
                    openBlock("}")
                }
            }
        }
    }

    override fun getDefault(): Writable = writable {
        UNREACHABLE("All methods from DefaultVisitor must be overridden.")
    }

    override fun visitTreeRule(rules: List<Rule>): Writable = rules.map { rule -> handleRule(rule) }.flatten()

    override fun visitErrorRule(error: Expression): Writable = writable {
        rustTemplate(
            "return Err(#{Error}(#{message:W}.into()));",
            "Error" to EndpointsModule.member("Error::endpoint_resolution"),
            "message" to error.accept(this@EndpointRuleSetVisitor),
        )
    }

    override fun visitEndpointRule(endpoint: Endpoint): Writable = writable {
        rustTemplate(
            "return Ok(#{Endpoint}::mutable(#{url:W}.parse().expect(\"invalid URI\")));",
            "Endpoint" to CargoDependency.SmithyHttp(runtimeConfig).asType().member("endpoint::Endpoint"),
            "url" to endpoint.url.accept(this@EndpointRuleSetVisitor),
        )
    }

    override fun visitLiteral(literal: Literal): Writable = writable {
        when (literal.type()) {
            is Type.Bool, is Type.Integer -> rustInline(literal.toString())
            is Type.String -> {
                val template = Template.fromString(literal.toString())
                template.accept(this@EndpointRuleSetVisitor).forEach {
                    it.invoke(this)
                }
            }
            else -> TODO("choked on unhandled literal $literal")
        }
    }

    override fun visitRef(reference: Reference): Writable = writable {
        rustInline("${reference.name.toRustName()}")
    }

    override fun visitIsSet(fn: Expression): Writable = writable {
        when (fn) {
            is GetAttr -> {
                // We don't assign here because these IsSet checks result in the checked value getting ignored
                rustInlineTemplate(
                    "#{expr:W}.is_some()",
                    "expr" to fn.accept(this@EndpointRuleSetVisitor),
                )
            }
            else -> {
                // We assign here because these IsSet checks almost always result in the checked value getting used
                rustInlineTemplate(
                    "let Some(#{expr:W}) = #{expr:W}",
                    "expr" to fn.accept(this@EndpointRuleSetVisitor),
                )
            }
        }
    }

    override fun visitNot(not: Expression): Writable = writable {
        when (not) {
            is IsSet -> rustInline("#W.is_none()", not.target.accept(this@EndpointRuleSetVisitor))
            else -> rustInline("!(#W)", not.accept(this@EndpointRuleSetVisitor))
        }
    }

    override fun visitBoolEquals(left: Expression, right: Expression): Writable = writable {
        rustInline(
            "#W == #W",
            left.accept(this@EndpointRuleSetVisitor),
            right.accept(this@EndpointRuleSetVisitor),
        )
    }

    override fun visitStringEquals(left: Expression, right: Expression): Writable = writable {
        rustInline(
            "#W == #W",
            left.accept(this@EndpointRuleSetVisitor),
            right.accept(this@EndpointRuleSetVisitor),
        )
    }

    override fun visitGetAttr(getAttr: GetAttr): Writable = writable {
        rustInline("#W", getAttr.target.accept(this@EndpointRuleSetVisitor))
        getAttr.path.forEach { part ->
            when (part) {
                is Part.Key -> rustInline(".${part.key().toRustName()}()")
                // we end up with Option<&&T>, we need to get to Option<&T>
                is Part.Index -> rustInline(".get(${part.index()}).cloned()")
                else -> TODO("are there other possible variants?")
            }
        }
    }

    override fun visitLibraryFunction(fn: FunctionDefinition, args: MutableList<Expression>): Writable {
        val writableArgs = args.iterator().let {
            writable {
                while (it.hasNext()) {
                    val nextExpr = it.next().accept(this@EndpointRuleSetVisitor)
                    rustInline("#W", nextExpr)
                    if (it.hasNext()) {
                        rustInline(", ")
                    }
                }
            }
        }

        return when (val id = fn.id) {
            // TODO(Zelda) move these AWS-specific fns into AWS codegen somehow
            "aws.isVirtualHostableS3Bucket" -> {
                writable {
                    rustTuple("#T", endpointLib.member("s3::is_virtual_hostable_s3_bucket")) {
                        rustInline("#W, &mut diagnostic_collector", writableArgs)
                    }
                }
            }

            "aws.partition" -> {
                writable {
                    rustTuple("partition_resolver.resolve_partition") {
                        rustInline("#W, &mut diagnostic_collector", writableArgs)
                    }
                    rustInline(".as_ref()")
                }
            }

            "aws.parseArn" -> {
                writable {
                    rustTuple("#T", endpointLib.member("arn::parse_arn")) {
                        rustInline("#W, &mut diagnostic_collector", writableArgs)
                    }
                    rustInline(".as_ref()")
                }
            }

            // TODO(Zelda) did I impl this correctly?
            "isValidHostLabel" -> {
                writable {
                    rustTuple("#T", endpointLib.member("host::is_valid_host_label")) {
                        rustInline("#W, &mut diagnostic_collector", writableArgs)
                    }
                }
            }

            "parseURL" -> {
                writable {
                    rustTuple("#T", endpointLib.member("parse_url::parse_url")) {
                        rustInline("#W, &mut diagnostic_collector", writableArgs)
                    }
                }
            }

            // TODO(Zelda) did I impl this correctly?
            "substring" -> {
                writable {
                    rustTuple("#T", endpointLib.member("substring::substring")) {
                        rustInline("#W, &mut diagnostic_collector", writableArgs)
                    }
                }
            }

            // TODO(Zelda) did I impl this correctly?
            "uriEncode" -> {
                writable {
                    rustTuple("#T", endpointLib.member("uri_encode::uri_encode")) {
                        rustInline("#W, &mut diagnostic_collector", writableArgs)
                    }
                    rustInline(".as_deref()")
                }
            }

            else -> TODO("unsupported builtin endpoints fn '$id'")
        }
    }

    override fun visitStaticTemplate(p0: String): Writable = writable {
        rustInline(p0)
    }

    override fun visitSingleDynamicTemplate(p0: Expression): Writable = writable {
        rust("#W;", p0.accept(this@EndpointRuleSetVisitor))
    }

    override fun visitStaticElement(p0: String): Writable = writable {
        // TODO(Zelda) How safe is this approach?
        // The start and end of templates ends up with extra quotes that we don't want.
        // This is because we use a string-builder API instead of the format! macro.
        // Hence, we must remove them. If input was the closing quote, we do nothing.
        if (p0 != "\"") {
            val output = p0
                // Remove leading quote
                .letIf(p0.startsWith("\"")) { p0.drop(1) }
                // Remove trailing quote
                .letIf(p0.endsWith("\"")) { p0.dropLast(1) }
                .dq()

            // if p0 starts with a double quote, remove it
            rust("out.push_str($output);")
        }
    }

    override fun visitDynamicElement(p0: Expression): Writable = writable {
        rust("out.push_str(#W);", p0.accept(this@EndpointRuleSetVisitor))
    }

    override fun startMultipartTemplate(): Writable = writable {
        rust(
            """
        {
            let mut out = String::new();
        """,
        )
    }

    override fun finishMultipartTemplate(): Writable = writable {
        rust("out }")
    }
}
