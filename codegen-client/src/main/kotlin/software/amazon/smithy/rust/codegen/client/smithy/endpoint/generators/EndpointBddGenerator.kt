/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators

import software.amazon.smithy.rulesengine.language.Endpoint
import software.amazon.smithy.rulesengine.language.evaluation.type.AnyType
import software.amazon.smithy.rulesengine.language.evaluation.type.ArrayType
import software.amazon.smithy.rulesengine.language.evaluation.type.BooleanType
import software.amazon.smithy.rulesengine.language.evaluation.type.OptionalType
import software.amazon.smithy.rulesengine.language.evaluation.type.StringType
import software.amazon.smithy.rulesengine.language.evaluation.type.Type
import software.amazon.smithy.rulesengine.language.syntax.Identifier
import software.amazon.smithy.rulesengine.language.syntax.expressions.Expression
import software.amazon.smithy.rulesengine.language.syntax.expressions.ExpressionVisitor
import software.amazon.smithy.rulesengine.language.syntax.expressions.Reference
import software.amazon.smithy.rulesengine.language.syntax.expressions.Template
import software.amazon.smithy.rulesengine.language.syntax.expressions.TemplateVisitor
import software.amazon.smithy.rulesengine.language.syntax.expressions.functions.Coalesce
import software.amazon.smithy.rulesengine.language.syntax.expressions.functions.FunctionDefinition
import software.amazon.smithy.rulesengine.language.syntax.expressions.functions.GetAttr
import software.amazon.smithy.rulesengine.language.syntax.expressions.functions.Ite
import software.amazon.smithy.rulesengine.language.syntax.expressions.functions.ParseUrl
import software.amazon.smithy.rulesengine.language.syntax.expressions.literal.Literal
import software.amazon.smithy.rulesengine.language.syntax.expressions.literal.LiteralVisitor
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameter
import software.amazon.smithy.rulesengine.language.syntax.parameters.ParameterType
import software.amazon.smithy.rulesengine.language.syntax.rule.Condition
import software.amazon.smithy.rulesengine.language.syntax.rule.EndpointRule
import software.amazon.smithy.rulesengine.language.syntax.rule.ErrorRule
import software.amazon.smithy.rulesengine.language.syntax.rule.NoMatchRule
import software.amazon.smithy.rulesengine.language.syntax.rule.Rule
import software.amazon.smithy.rulesengine.language.syntax.rule.RuleValueVisitor
import software.amazon.smithy.rulesengine.traits.EndpointBddTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.Context
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointTypesGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointsLib
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.Types
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.memberName
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen.BddExpressionGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen.ExpressionGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen.Ownership
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rustName
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.util.dq

/**
 * Generator for BDD-based endpoint resolvers
 */
class EndpointBddGenerator(
    private val codegenContext: ClientCodegenContext,
    private val bddTrait: EndpointBddTrait,
    private val stdlib: List<CustomRuntimeFunction>,
) {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val allRefs = AnnotatedRefs.from(bddTrait, runtimeConfig)

    companion object {
        private const val COND_PREFIX = "Cond"
        private const val RESULT_PREFIX = "Result"
        private const val BINDING_PREFIX = "binding_"
        private const val CONDITION_FN_PREFIX = "cond_"
    }

    private data class GenerationContext(
        val typeGenerator: EndpointTypesGenerator,
        val registry: FunctionRegistry,
        val context: Context,
        val fnsUsed: List<CustomRuntimeFunction>,
        val conditionScope: Map<String, Writable>,
        val additionalArgsSignature: List<Writable>,
        val conditionCount: Int,
        val resultCount: Int,
    )

    private fun generateEnumVariants(
        count: Int,
        prefix: String,
    ): String = (0 until count).joinToString(",\n    ") { "$prefix$it" }

    private fun generateMatchArms(
        count: Int,
        template: (Int) -> String,
    ): String = (0 until count).joinToString(",\n") { idx -> template(idx) }

    /**
     * Main entrypoint for creating a BDD based endpoint resolver
     */
    private fun generateBddResolverImpl(writer: RustWriter) {
        val genContext = prepareGenerationContext()

        writer.rustTemplate(
            """
            #{ResolverStruct:W}
            #{ConditionFn:W}
            #{ResultEndpoint:W}
            #{Nodes:W}
            #{ConditionContext:W}
            """,
            "ResolverStruct" to generateResolverStruct(genContext),
            "ConditionFn" to generateConditionFn(genContext),
            "ResultEndpoint" to generateResultEndpoint(genContext),
            "Nodes" to generateNodeArray(),
            "ConditionContext" to generateContextStruct(),
        )
    }

    private fun prepareGenerationContext(): GenerationContext {
        val conditionCount = bddTrait.conditions.size
        val resultCount = bddTrait.results.size
        val typeGenerator = EndpointTypesGenerator.fromContext(codegenContext)
        val registry = FunctionRegistry(stdlib)
        val context = Context(registry, runtimeConfig, isBddMode = true)

        // Render conditions to a dummy writer to populate the function registry
        bddTrait.conditions.forEach { cond ->
            val bddExpressionGenerator =
                BddExpressionGenerator(cond, Ownership.Borrowed, context, allRefs, codegenContext)
            bddExpressionGenerator.generateCondition(cond)(
                RustWriter.root(),
            )
        }

        val fnsUsed = registry.fnsUsed()
        val additionalArgsSignature = fnsUsed.mapNotNull { it.additionalArgsSignatureBdd() }

        val conditionScope =
            bddTrait.conditions.withIndex().associate { (idx, cond) ->
                val bddExpressionGenerator =
                    BddExpressionGenerator(cond, Ownership.Borrowed, context, allRefs, codegenContext)
                "$CONDITION_FN_PREFIX$idx" to bddExpressionGenerator.generateCondition(cond)
            }

        return GenerationContext(
            typeGenerator, registry, context, fnsUsed, conditionScope,
            additionalArgsSignature, conditionCount, resultCount,
        )
    }

    private fun generateResolverStruct(genContext: GenerationContext) =
        writable {
            rustTemplate(
                """
                ##[derive(Debug)]
                /// The default endpoint resolver.
                pub struct DefaultResolver {
                    #{CustomFields}
                }

                 impl Default for DefaultResolver {
                    fn default() -> Self {
                        Self::new()
                    }
                 }

                impl DefaultResolver {
                    /// Create a new DefaultResolver
                    pub fn new() -> Self {
                        Self {
                            #{CustomFieldsInit}
                        }
                    }

                    ##[allow(clippy::needless_borrow)]
                    pub(crate) fn evaluate_fn(
                        &self,
                    ) -> impl for<'a> FnMut(
                        &ConditionFn,
                        &'a Params,
                        &mut ConditionContext<'a>,
                        &mut #{DiagnosticCollector},
                    ) -> bool
                           + '_ {
                        move |cond, params, context, _diagnostic_collector| {
                            cond.evaluate(
                                params,
                                context,
                                #{CustomFieldsArgs}
                                _diagnostic_collector,
                            )
                        }
                    }

                    fn resolve_endpoint<'a>(&'a self, params: &'a #{Params}) -> #{Result}<#{SmithyEndpoint}, #{BoxError}> {
                        let mut diagnostic_collector = #{DiagnosticCollector}::new();
                        let mut condition_context = ConditionContext::default();
                        let result = #{EvaluateBdd}(
                            &NODES,
                            &CONDITIONS,
                            &RESULTS,
                            ${bddTrait.bdd.rootRef},
                            params,
                            &mut condition_context,
                            &mut diagnostic_collector,
                            self.evaluate_fn(),
                        );

                        match result {
                            #{Some}(endpoint) => match endpoint.to_endpoint(params, &condition_context) {
                                Ok(ep) => Ok(ep),
                                Err(err) => Err(Box::new(err)),
                            },
                            #{None} => #{Err}(Box::new(#{ResolveEndpointError}::message("No endpoint rule matched"))),
                        }
                    }
                }

                impl #{ServiceSpecificEndpointResolver} for DefaultResolver {
                    fn resolve_endpoint<'a>(&'a self, params: &'a #{Params}) -> #{EndpointFuture}<'a> {
                        let result = self.resolve_endpoint(params);

                        #{EndpointFuture}::ready(result)
                    }
                }
                """,
                *preludeScope,
                "BoxError" to RuntimeType.boxError(runtimeConfig),
                "CustomFields" to
                    writable {
                        genContext.fnsUsed.mapNotNull { it.structFieldBdd() }.forEach { rust("#W,", it) }
                    },
                "CustomFieldsArgs" to
                    writable {
                        genContext.fnsUsed.mapNotNull { it.additionalArgsInvocation("self") }
                            .forEach { rust("#W,", it) }
                    },
                "CustomFieldsInit" to
                    writable {
                        genContext.fnsUsed.mapNotNull { it.structFieldInitBdd() }.forEach { rust("#W,", it) }
                    },
                "DiagnosticCollector" to EndpointsLib.DiagnosticCollector,
                "EvaluateBdd" to EndpointsLib.evaluateBdd,
                "Params" to genContext.typeGenerator.paramsStruct(),
                "ResolveEndpointError" to Types(runtimeConfig).resolveEndpointError,
                "ServiceSpecificEndpointResolver" to codegenContext.serviceSpecificEndpointResolver(),
                "SmithyEndpoint" to Types(runtimeConfig).smithyEndpoint,
                *Types(runtimeConfig).toArray(),
            )
        }

    private fun generateConditionFn(genContext: GenerationContext) =
        writable {
            rustTemplate(
                """
                ##[derive(Debug)]
                pub(crate) enum ConditionFn {
                    ${generateEnumVariants(genContext.conditionCount, COND_PREFIX)}
                }

                impl ConditionFn {
                    ##[allow(unused_variables, unused_parens, clippy::double_parens,
                        clippy::useless_conversion, clippy::bool_comparison, clippy::comparison_to_empty,
                        clippy::needless_borrow, clippy::useless_asref, )]
                    fn evaluate<'a>(&self, params: &'a Params, context: &mut ConditionContext<'a>#{AdditionalArgsSigPrefix}#{AdditionalArgsSig}, _diagnostic_collector: &mut #{DiagnosticCollector}) -> bool {
                        // Param bindings
                        #{ParamBindings:W}

                        // Non-Param references
                        #{NonParamRefBindings:W}
                        match self {
                            ${generateMatchArms(genContext.conditionCount) { idx -> "Self::$COND_PREFIX$idx => {#{$CONDITION_FN_PREFIX$idx:W}}" }}
                        }
                    }
                }

                const CONDITIONS: [ConditionFn; ${genContext.conditionCount}] = [
                    ${generateEnumVariants(genContext.conditionCount, "ConditionFn::$COND_PREFIX")}
                ];
                """,
                *preludeScope,
                "AdditionalArgsSig" to
                    writable {
                        genContext.additionalArgsSignature.forEachIndexed { i, it ->
                            if (i > 0) rust(", ")
                            rust("#W", it)
                        }
                    },
                "AdditionalArgsSigPrefix" to writable { if (genContext.additionalArgsSignature.isNotEmpty()) rust(", ") },
                "DiagnosticCollector" to EndpointsLib.DiagnosticCollector,
                "NonParamRefBindings" to generateNonParamReferences(),
                "ParamBindings" to generateParamBindings(),
                *genContext.conditionScope.toList().toTypedArray(),
            )
        }

    private fun generateResultEndpoint(genContext: GenerationContext) =
        writable {
            rustTemplate(
                """
                ##[derive(Debug, Clone)]
                enum ResultEndpoint {
                    ${generateEnumVariants(genContext.resultCount, RESULT_PREFIX)}
                }

                impl<'a> ResultEndpoint {
                    ##[allow(unused_variables, clippy::useless_asref)]
                    fn to_endpoint(&self, params: &'a Params, context: &ConditionContext<'a>) -> #{Result}<#{Endpoint}, #{ResolveEndpointError}> {
                        match self {
                            #{ResultArms:W}
                        }
                    }
                }

                const RESULTS: [ResultEndpoint; ${genContext.resultCount}] = [
                    ${generateEnumVariants(genContext.resultCount, "ResultEndpoint::$RESULT_PREFIX")}
                ];
                """,
                *preludeScope,
                "Endpoint" to Types(runtimeConfig).smithyEndpoint,
                "ResolveEndpointError" to Types(runtimeConfig).resolveEndpointError,
                "ResultArms" to generateResultArmsWithBindings(genContext.context),
            )
        }

    /**
     * Generates bindings for the Params attached to the `endpointBddTrait`
     */
    private fun generateParamBindings(forResults: Boolean = false) =
        writable {
            bddTrait.parameters.toList().forEach { param ->
                rust(generateParamBinding(param, forResults))
            }
        }

    private fun generateParamBinding(
        param: Parameter,
        forResults: Boolean,
    ): String {
        val memberName = param.memberName()

        if (!forResults) {
            return "let $memberName = &params.$memberName;"
        }

        return when {
            param.isRequired && (param.type == ParameterType.STRING || param.type == ParameterType.STRING_ARRAY) ->
                "let $memberName = &params.$memberName;"

            param.isRequired ->
                "let $memberName = params.$memberName;"

            isStringRef(memberName) ->
                "let $memberName = params.$memberName.as_ref().map(|s| s.clone()).unwrap_or_default();"

            else ->
                "let $memberName = params.$memberName.expect(\"Guaranteed to have a value by earlier checks.\");"
        }
    }

    private fun isStringRef(memberName: String): Boolean {
        val stringRefs =
            allRefs.filter { ref -> ref.runtimeType == RuntimeType.String }
                .map { ref -> ref.name }
        return stringRefs.contains(memberName)
    }

    /**
     * Generates references that do not come from the trait params. These can be set
     * as part of the evaluation of a Condition. They all start as `None` since they may
     * never be set.
     */
    private fun generateNonParamReferences() =
        writable {
            val varRefs = allRefs.variableRefs()
            varRefs.forEach {
                rust("let ${it.value.name} = &mut context.${it.value.name};")
            }
        }

    /**
     * Generate the arms of the `Result` match statement with bindings inlined.
     */
    private fun generateResultArmsWithBindings(context: Context) =
        writable {
            val visitor = RuleVisitor(context)
            bddTrait.results.forEachIndexed { idx, rule ->
                // Skip NoMatchRule (index 0), it doesn't support visitor pattern
                if (rule is NoMatchRule) {
                    rustTemplate(
                        "Self::Result$idx => #{Err}(#{ResolveEndpointError}::message(\"No endpoint rule matched\")),\n",
                        *preludeScope,
                        "ResolveEndpointError" to Types(runtimeConfig).resolveEndpointError,
                    )
                } else {
                    val usedVars = collectUsedVariables(rule)
                    val bindings = generateBindingsForVariables(usedVars)
                    val ruleOutput = rule.accept(visitor)
                    rustTemplate(
                        """
                        Self::Result$idx => {
                            #{bindings:W}
                            #{output:W}
                        },
                        """,
                        "bindings" to bindings,
                        "output" to ruleOutput,
                    )
                }
            }
        }

    /**
     * Collect all variables (params and context refs) used in a rule.
     */
    private fun collectUsedVariables(rule: Rule): Set<String> {
        val usedVars = mutableSetOf<String>()

        // Forward declaration via late init
        lateinit var collectFromExpression: (Expression) -> Unit
        lateinit var collectFromLiteral: (Literal) -> Unit

        collectFromLiteral = { literal ->
            literal.accept(
                object : LiteralVisitor<Unit> {
                    override fun visitBoolean(b: Boolean) {}

                    override fun visitString(value: Template) {
                        // For templates, we need to extract the dynamic parts
                        val parts =
                            value.accept(
                                object :
                                    TemplateVisitor<Expression?> {
                                    override fun visitStaticTemplate(value: String) = null

                                    override fun visitSingleDynamicTemplate(expr: Expression) = expr

                                    override fun visitStaticElement(str: String) = null

                                    override fun visitDynamicElement(expr: Expression) = expr

                                    override fun startMultipartTemplate() = null

                                    override fun finishMultipartTemplate() = null
                                },
                            )
                        parts.forEach { if (it != null) collectFromExpression(it) }
                    }

                    override fun visitRecord(members: MutableMap<Identifier, Literal>) {
                        members.values.forEach { collectFromLiteral(it) }
                    }

                    override fun visitTuple(members: MutableList<Literal>) {
                        members.forEach { collectFromLiteral(it) }
                    }

                    override fun visitInteger(value: Int) {}
                },
            )
        }

        collectFromExpression = { expr ->
            expr.accept(
                object : ExpressionVisitor<Unit> {
                    override fun visitLiteral(literal: Literal) {
                        collectFromLiteral(literal)
                    }

                    override fun visitRef(reference: Reference) {
                        usedVars.add(reference.name.rustName())
                    }

                    override fun visitGetAttr(getAttr: GetAttr) {
                        collectFromExpression(getAttr.target)
                    }

                    override fun visitIsSet(target: Expression) {
                        collectFromExpression(target)
                    }

                    override fun visitNot(not: Expression) {
                        collectFromExpression(not)
                    }

                    override fun visitBoolEquals(
                        left: Expression,
                        right: Expression,
                    ) {
                        collectFromExpression(left)
                        collectFromExpression(right)
                    }

                    override fun visitStringEquals(
                        left: Expression,
                        right: Expression,
                    ) {
                        collectFromExpression(left)
                        collectFromExpression(right)
                    }

                    override fun visitLibraryFunction(
                        fn: FunctionDefinition,
                        args: MutableList<Expression>,
                    ) {
                        args.forEach { collectFromExpression(it) }
                    }
                },
            )
        }

        when (rule) {
            is EndpointRule -> {
                val endpoint = rule.endpoint
                collectFromExpression(endpoint.url)
                endpoint.headers.values.forEach { values -> values.forEach { collectFromExpression(it) } }
                endpoint.properties.values.forEach { collectFromExpression(it) }
            }

            is ErrorRule -> {
                collectFromExpression(rule.error)
            }
        }

        return usedVars
    }

    /**
     * Generate bindings for the specified variables.
     */
    private fun generateBindingsForVariables(usedVars: Set<String>) =
        writable {
            // Generate param bindings for used params
            bddTrait.parameters.toList().forEach { param ->
                val memberName = param.memberName()
                if (usedVars.contains(memberName)) {
                    rust(generateParamBinding(param, forResults = true) + "\n")
                }
            }

            // Generate non-param reference bindings for used refs
            val varRefs = allRefs.variableRefs()
            varRefs.values.forEachIndexed { idx, ref ->
                val rustName = ref.name
                if (usedVars.contains(rustName)) {
                    if (allRefs.filter { it.runtimeType == RuntimeType.document(runtimeConfig) }
                            .map { it.name }.contains(rustName)
                    ) {
                        rust(
                            """
                            let binding_$idx = context.$rustName.as_ref().map(|s| s.clone()).unwrap_or_default();
                            let $rustName = binding_$idx.as_string().unwrap_or_default();

                            """.trimIndent(),
                        )
                    } else {
                        rust("let $rustName = context.$rustName.as_ref().map(|s| s.clone()).expect(\"Guaranteed to have a value by earlier checks.\");\n")
                    }
                }
            }
        }

    /**
     * The rule visitor used to construct the match arms for `ResultEndpoint`
     */
    inner class RuleVisitor(private val context: Context) : RuleValueVisitor<Writable> {
        override fun visitTreeRule(rules: List<Rule>): Writable {
            throw UnsupportedOperationException("Tree rules not supported in BDD endpoint resolver")
        }

        override fun visitErrorRule(error: Expression): Writable =
            writable {
                rustTemplate(
                    "#{Err}(#{ResolveEndpointError}::message(#{message:W}))",
                    *preludeScope,
                    "ResolveEndpointError" to Types(runtimeConfig).resolveEndpointError,
                    "message" to ExpressionGenerator(Ownership.Owned, context).generate(error),
                )
            }

        override fun visitEndpointRule(endpoint: Endpoint): Writable =
            writable {
                rustTemplate("#{Ok}(#{endpoint:W})", *preludeScope, "endpoint" to generateEndpoint(endpoint))
            }
    }

    /**
     * Generates an `Endpoint` using an `Endpoint::builder()` and the same `Params`/`ConditionContext`
     * used in Condition evaluation.
     */
    private fun generateEndpoint(endpoint: Endpoint): Writable {
        val registry = FunctionRegistry(stdlib)
        val context = Context(registry, runtimeConfig, isBddMode = true)
        val generator = ExpressionGenerator(Ownership.Owned, context)
        val url = generator.generate(endpoint.url)
        val headers = endpoint.headers.mapValues { entry -> entry.value.map { generator.generate(it) } }
        val properties = endpoint.properties.mapValues { entry -> generator.generate(entry.value) }
        return writable {
            rustTemplate(
                "#{SmithyEndpoint}::builder().url(#{url:W})",
                "SmithyEndpoint" to Types(runtimeConfig).smithyEndpoint,
                "url" to url,
            )
            headers.forEach { (name, values) -> values.forEach { rust(".header(${name.dq()}, #W)", it) } }
            properties.forEach { (name, value) -> rust(".property(${name.toString().dq()}, #W)", value) }
            rust(".build()")
        }
    }

    /**
     * Generate the `ConditionContext` struct that is passed to each condition evaluation.
     * This allows passing mutable state between the conditions.
     */
    private fun generateContextStruct() =
        writable {
            val varRefs = allRefs.variableRefs()
            val registry = FunctionRegistry(stdlib)

            var memberDefs =
                varRefs.map { entry ->
                    val ref = entry.value
                    val rustType = inferContextMemberType(ref, registry)
                    formatContextMember(ref.name, rustType)
                }.joinToString(",\n")

            if (memberDefs.isNotEmpty()) {
                memberDefs += ","
            }

            rustTemplate(
                """
                // These are all optional since they are set by conditions and will
                // all be unset when we start evaluation
                ##[derive(Default)]
                ##[allow(unused_lifetimes)]
                pub(crate) struct ConditionContext<'a> {
                    $memberDefs
                    // Sometimes none of the members reference the lifetime, this makes it still valid
                    phantom: std::marker::PhantomData<&'a ()>
                }
                """.trimIndent(),
            )
        }

    private fun inferContextMemberType(
        ref: AnnotatedRefs.AnnotatedRef,
        registry: FunctionRegistry,
    ): RuntimeType {
        val fn = ref.condition?.function ?: throw IllegalArgumentException("Variable ref missing condition")
        val type = if (ref.type is OptionalType) (ref.type as OptionalType).inner() else ref.type

        return when {
            fn is GetAttr -> matchRuleTypeToRustType(fn.type(), runtimeConfig)
            fn is Coalesce -> matchRuleTypeToRustType(fn.type(), runtimeConfig)
            fn is Ite -> matchRuleTypeToRustType(fn.type(), runtimeConfig)
            type is StringType -> RuntimeType.String
            type is BooleanType -> RuntimeType.Bool
            type is ArrayType -> RuntimeType.typedVec(RuntimeType.String)
            else ->
                registry.fnFor(fn.functionDefinition.id)?.returnType()
                    ?: throw IllegalArgumentException("Unsupported reference type $ref")
        }
    }

    private fun formatContextMember(
        name: String,
        rustType: RuntimeType,
    ): String {
        return if (rustType.namespace.startsWith("::std::option::Option<")) {
            "pub(crate) $name: ${rustType.render()}"
        } else {
            "pub(crate) $name: Option<${rustType.render()}>"
        }
    }

    /**
     * Generate the array of `BddNode`s that model the evaluation DAG
     */
    private fun generateNodeArray() =
        writable {
            val bdd = bddTrait.bdd
            val nodes = mutableListOf<String>()
            val length = bdd.nodeCount

            bdd.getNodes { condIdx, high, low ->
                nodes.add("#{BddNode} { condition_index: $condIdx, high_ref: $high, low_ref: $low }")
            }

            rustTemplate(
                "const NODES: [#{BddNode}; $length] = [\n${nodes.joinToString(",\n")}\n];",
                "BddNode" to EndpointsLib.bddNode,
            )
        }

    /**
     * Used for generating tests
     */
    fun generateBddResolver(): RuntimeType {
        return RuntimeType.forInlineFun("DefaultResolver", ClientRustModule.Config.endpoint) {
            generateBddResolverImpl(this)
        }
    }
}

/**
 * Maps Smithy rule types to Rust runtime types
 */
class RustTypeMapper(private val runtimeConfig: RuntimeConfig) {
    fun mapRuleType(ruleType: Type): RuntimeType =
        when (ruleType) {
            is StringType -> RuntimeType.String
            is BooleanType -> RuntimeType.Bool
            is ArrayType -> RuntimeType.typedVec(mapRuleType(ruleType.member))
            is OptionalType -> RuntimeType.typedOption(mapRuleType(ruleType.inner()))
            is AnyType -> RuntimeType.document(runtimeConfig)
            else -> throw IllegalArgumentException("Unsupported rules type: $ruleType")
        }

    fun mapParameterType(paramType: ParameterType): RuntimeType =
        when (paramType) {
            ParameterType.STRING -> RuntimeType.String
            ParameterType.STRING_ARRAY -> RuntimeType.typedVec(RuntimeType.String)
            ParameterType.BOOLEAN -> RuntimeType.Bool
        }
}

fun matchRuleTypeToRustType(
    ruleType: Type,
    runtimeConfig: RuntimeConfig,
): RuntimeType = RustTypeMapper(runtimeConfig).mapRuleType(ruleType)

/**
 * Container for annotated references, these are the variables the condition evaluation can potentially
 * refer to. They come in two variants: Parameters (which are immutable), and Variables (which are all Optional,
 * begin as None, and might be set by a condition during evaluation).
 */
class AnnotatedRefs(
    private val refs: Map<String, AnnotatedRef>,
) {
    enum class RefType {
        Parameter,
        Variable,
    }

    data class AnnotatedRef(
        val name: String,
        val refType: RefType,
        val isOptional: Boolean,
        val runtimeType: RuntimeType,
        // These two are only present when RefType == Variable
        val condition: Condition?,
        val type: Type?,
    )

    operator fun get(name: String): AnnotatedRef? = refs[name]

    fun filter(predicate: (AnnotatedRef) -> Boolean): List<AnnotatedRef> = refs.values.filter(predicate)

    fun map(transform: (AnnotatedRef) -> String): List<String> = refs.values.map(transform)

    fun variableRefs(): Map<String, AnnotatedRef> = refs.filter { entry -> entry.value.refType == RefType.Variable }

    fun paramRefs(): Map<String, AnnotatedRef> = refs.filter { entry -> entry.value.refType == RefType.Parameter }

    fun allRefs(): Map<String, AnnotatedRef> = refs

    companion object {
        fun from(
            bddTrait: EndpointBddTrait,
            runtimeConfig: RuntimeConfig,
        ): AnnotatedRefs {
            val refs = mutableMapOf<String, AnnotatedRef>()
            val typeMapper = RustTypeMapper(runtimeConfig)

            bddTrait.parameters.forEach { param ->
                val runtimeType =
                    param.type?.let { typeMapper.mapParameterType(it) }
                        ?: throw IllegalArgumentException("Unsupported parameter type ${param.type}")

                refs[param.memberName()] =
                    AnnotatedRef(
                        param.memberName(),
                        RefType.Parameter,
                        !param.isRequired,
                        runtimeType,
                        null,
                        null,
                    )
            }

            bddTrait.conditions.forEach { cond ->
                val result = cond.result.orElse(null)
                if (result !== null) {
                    val returnType =
                        if (cond.function.functionDefinition.returnType is OptionalType) {
                            (cond.function.functionDefinition.returnType as OptionalType).inner()
                        } else {
                            cond.function.functionDefinition.returnType
                        }

                    val runtimeType =
                        when {
                            cond.function is Coalesce -> matchRuleTypeToRustType(cond.function.type(), runtimeConfig)
                            cond.function is Ite -> matchRuleTypeToRustType(cond.function.type(), runtimeConfig)
                            cond.function is GetAttr -> matchRuleTypeToRustType(cond.function.type(), runtimeConfig)
                            cond.function is ParseUrl -> EndpointsLib.url()
                            cond.function.name == "aws.partition" -> EndpointsLib.partition(runtimeConfig)
                            cond.function.name == "aws.parseArn" -> EndpointsLib.arn()
                            else -> matchRuleTypeToRustType(returnType, runtimeConfig)
                        }

                    refs[result.rustName()] =
                        AnnotatedRef(result.rustName(), RefType.Variable, true, runtimeType, cond, returnType)
                }
            }

            return AnnotatedRefs(refs)
        }
    }
}
