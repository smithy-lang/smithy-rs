/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

import software.amazon.smithy.rulesengine.language.Endpoint
import software.amazon.smithy.rulesengine.language.EndpointRuleSet
import software.amazon.smithy.rulesengine.language.eval.Type
import software.amazon.smithy.rulesengine.language.syntax.expr.Expression
import software.amazon.smithy.rulesengine.language.syntax.expr.Reference
import software.amazon.smithy.rulesengine.language.syntax.fn.Function
import software.amazon.smithy.rulesengine.language.syntax.fn.IsSet
import software.amazon.smithy.rulesengine.language.syntax.rule.Condition
import software.amazon.smithy.rulesengine.language.syntax.rule.Rule
import software.amazon.smithy.rulesengine.language.visit.RuleValueVisitor
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen.ExpressionGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen.Ownership
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen.Scope
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.asType
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.orNull

abstract class CustomRuntimeFunction {
    abstract val id: String

    /** Initialize the struct field to a default value */
    abstract fun structFieldInit(): Writable?

    /** The argument slot of the runtime function. MUST NOT include `,`
     * e.g `partition_data: &PartitionData`
     * */
    abstract fun additionalArgsSignature(): Writable?

    /**
     * A writable that passes additional args from `self` into the function. Must match the order of additionalArgsSignature
     */
    abstract fun additionalArgsInvocation(self: String): Writable?

    /**
     * Any additional struct fields this runtime function adds to the resolver
     */
    abstract fun structField(): Writable?

    /**
     * Invoking the runtime function—(parens / args not needed) `$fn`
     */
    abstract fun usage(): Writable
}

class FunctionRegistry(private val functions: List<CustomRuntimeFunction>) {
    private var usedFunctions = mutableSetOf<CustomRuntimeFunction>()
    fun fnFor(id: String): CustomRuntimeFunction? =
        functions.firstOrNull { it.id == id }?.also { usedFunctions.add(it) }

    fun fnsUsed(): List<CustomRuntimeFunction> = usedFunctions.toList()
}

/**
 * Generate an endpoint resolver struct. The struct may contain extras resulting from the usage of functions e.g. partition function
 * 1. resolver configuration (e.g. a custom partitions.json)
 * 2. extra function arguments in the resolver
 * 3. the runtime type of the library function
 *
 * ```rust
 * pub struct DefaultResolver {
 *   partition: PartitionResolver
 * }
 *
 * fn resolve_endpoint(params: crate::endpoint::Params,
 *
 */
class EndpointResolverGenerator(stdlib: List<CustomRuntimeFunction>, private val runtimeConfig: RuntimeConfig) {
    private val registry: FunctionRegistry = FunctionRegistry(stdlib)
    // first, make a custom RustWriter and generate the interior of the resolver into it.
    // next, since we've now captured what runtime functions are required, generate the container

    private val smithyHttpEndpoint = CargoDependency.smithyHttp(runtimeConfig).asType().member("endpoint")
    private val smithyTypesEndpoint = CargoDependency.smithyTypes(runtimeConfig).asType().member("endpoint")
    private val codegenScope = arrayOf(
        "endpoint" to smithyHttpEndpoint,
        "SmithyEndpoint" to smithyTypesEndpoint.member("Endpoint"),
        "EndpointError" to smithyHttpEndpoint.member("ResolveEndpointError"),
        "DiagnosticCollector" to endpointsLib("diagnostic").asType().member("DiagnosticCollector"),
    )

    private val ParamsName = "_params"

    companion object {
        const val DiagnosticCollector = "_diagnostic_collector"
    }

    fun generateResolverStruct(endpointRuleSet: EndpointRuleSet): RuntimeType {
        check(endpointRuleSet.rules.isNotEmpty()) { "EndpointRuleset must contain at least one rule." }
        val innerWriter = RustWriter.root()
        resolverFnBody(endpointRuleSet, registry)(innerWriter)
        val fnsUsed = registry.fnsUsed()
        return RuntimeType.forInlineFun("DefaultResolver", EndpointsModule) {
            rustTemplate(
                """
                /// The default endpoint resolver
                pub struct DefaultResolver {
                    #{custom_fields:W}
                }

                impl DefaultResolver {
                    /// Create a new endpoint resolver with default settings
                    pub fn new() -> Self { 
                        Self { #{custom_fields_init:W} }
                    }
                }

                impl #{endpoint}::ResolveEndpoint<#{Params}> for DefaultResolver {

                    fn resolve_endpoint(&self, params: &Params) -> #{endpoint}::Result {
                        let mut diagnostic_collector = #{DiagnosticCollector}::new();
                        #{resolver_fn}(params, &mut diagnostic_collector, #{additional_args})
                            .map_err(|err|err.with_source(diagnostic_collector.take_last_error()))
                    }
                }
                """,
                "custom_fields" to fnsUsed.mapNotNull { it.structField() }.join(","),
                "custom_fields_init" to fnsUsed.mapNotNull { it.structFieldInit() }.join(","),
                "Params" to EndpointParamsGenerator(endpointRuleSet.parameters).paramsStruct(),
                "additional_args" to fnsUsed.mapNotNull { it.additionalArgsInvocation("self") }.join(","),
                "resolver_fn" to resolverFn(endpointRuleSet, registry, fnsUsed),
                *codegenScope,
            )
        }
    }

    private fun resolverFn(
        endpointRuleSet: EndpointRuleSet,
        registry: FunctionRegistry,
        fnsUsed: List<CustomRuntimeFunction>,
    ): RuntimeType {
        return RuntimeType.forInlineFun("resolve_endpoint", EndpointsImpl) {
            rustTemplate(
                """
                pub(super) fn resolve_endpoint($ParamsName: &#{Params}, $DiagnosticCollector: &mut #{DiagnosticCollector}, #{additional_args}) -> #{endpoint}::Result {
                 #{body:W}
                }

                """,
                *codegenScope,
                "Params" to EndpointParamsGenerator(endpointRuleSet.parameters).paramsStruct(),
                "additional_args" to fnsUsed.mapNotNull { it.additionalArgsSignature() }.join(","),
                "body" to resolverFnBody(endpointRuleSet, registry),
            )
        }
    }

    private fun resolverFnBody(endpointRuleSet: EndpointRuleSet, registry: FunctionRegistry) = writable {
        val scope = Scope.empty()
        endpointRuleSet.parameters.toList().forEach {
            Attribute.AllowUnused.render(this)
            rust("let ${it.memberName()} = &$ParamsName.${it.memberName()};")
        }
        generateRulesList(endpointRuleSet.rules, scope)(this)
    }

    private fun generateRulesList(rules: List<Rule>, scope: Scope) = writable {
        rules.forEach { rule ->
            rule.documentation.orNull()?.also { docs(it, newlinePrefix = "//") }
            generateRule(rule, scope)(this)
        }
        if (rules.last().conditions.isNotEmpty()) {
            rustTemplate(
                """return Err(#{EndpointError}::message(format!("No rules matched these parameters. This is a bug. {:?}", $ParamsName)))""",
                *codegenScope,
            )
        }
    }

    private fun generateRule(rule: Rule, scope: Scope): Writable {
        return generateRuleInternal(rule, rule.conditions, scope)
    }

    private fun Condition.conditionalFunction(): Expression {
        return when (val fn = this.fn) {
            is IsSet -> fn.target
            else -> fn
        }
    }

    private var nameIdx = 0
    private fun nameFor(expr: Expression): String {
        nameIdx += 1
        return "_" + when (expr) {
            is Reference -> expr.name.rustName()
            else -> "var_$nameIdx"
        }
    }

    private fun contextFor(scope: Scope) = Context(scope, registry, runtimeConfig)

    private fun generateRuleInternal(rule: Rule, conditions: List<Condition>, scope: Scope): Writable {
        if (conditions.isEmpty()) {
            return rule.accept(RuleVisitor(scope))
        } else {
            val condition = conditions.first()
            val rest = conditions.drop(1)
            return {
                val generator = ExpressionGenerator(Ownership.Borrowed, contextFor(scope))
                val fn = condition.conditionalFunction()
                val condName = condition.result.orNull()?.rustName() ?: nameFor(condition.conditionalFunction())

                when {
                    fn.type() is Type.Option || (fn as Function).name == "substring" -> rustTemplate(
                        "if let Some($condName) = #{target:W} { #{next:W} }",
                        "target" to generator.generate(fn),
                        "next" to generateRuleInternal(rule, rest, scope.withMember(condName, fn)),
                    )

                    condition.result.isPresent -> {
                        rustTemplate(
                            """
                            let $condName = #{target:W};
                            #{next:W}
                            """,
                            "target" to generator.generate(fn),
                            "next" to generateRuleInternal(rule, rest, scope.withMember(condName, fn)),
                        )
                    }

                    else -> {
                        rustTemplate(
                            """if #{target:W} { #{next:W} }""",
                            "target" to generator.generate(fn),
                            "next" to generateRuleInternal(rule, rest, scope),
                        )
                    }
                }
            }
        }
    }

    inner class RuleVisitor(private val scope: Scope) : RuleValueVisitor<Writable> {
        override fun visitTreeRule(rules: List<Rule>) = generateRulesList(rules, scope)

        override fun visitErrorRule(error: Expression) = writable {
            rustTemplate(
                "return Err(#{EndpointError}::message(#{message:W}));",
                *codegenScope,
                "message" to ExpressionGenerator(Ownership.Owned, contextFor(scope)).generate(error),
            )
        }

        override fun visitEndpointRule(endpoint: Endpoint): Writable = writable {
            rust("return Ok(#W);", generateEndpoint(endpoint, scope))
        }
    }

    /**
     * generate the rust code for `[endpoint]`
     */
    internal fun generateEndpoint(endpoint: Endpoint, scope: Scope): Writable {
        val generator = ExpressionGenerator(Ownership.Owned, contextFor(scope))
        val url = generator.generate(endpoint.url)
        val headers = endpoint.headers.mapValues { entry -> entry.value.map { generator.generate(it) } }
        val properties = endpoint.properties.mapValues { entry -> generator.generate(entry.value) }
        return writable {
            rustTemplate("#{SmithyEndpoint}::builder().url(#{url:W})", *codegenScope, "url" to url)
            headers.forEach { (name, values) -> values.forEach { rust(".header(${name.dq()}, #W)", it) } }
            properties.forEach { (name, value) -> rust(".property(${name.asString().dq()}, #W)", value) }
            rust(".build()")
        }
    }
}
