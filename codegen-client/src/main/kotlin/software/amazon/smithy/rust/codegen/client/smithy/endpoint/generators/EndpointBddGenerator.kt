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
import software.amazon.smithy.rulesengine.language.syntax.expressions.Expression
import software.amazon.smithy.rulesengine.language.syntax.expressions.functions.GetAttr
import software.amazon.smithy.rulesengine.language.syntax.expressions.functions.ParseUrl
import software.amazon.smithy.rulesengine.language.syntax.parameters.ParameterType
import software.amazon.smithy.rulesengine.language.syntax.rule.Condition
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
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.toPascalCase

/**
 * Generator for BDD-based endpoint resolvers
 */
class EndpointBddGenerator(
    private val codegenContext: ClientCodegenContext,
    private val bddTrait: EndpointBddTrait,
    private val stdlib: List<CustomRuntimeFunction>,
) {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val serviceName = codegenContext.serviceShape.id.name.toPascalCase()
    private val allRefs = AnnotatedRefs.from(bddTrait)

    fun generateBddResolver(): RuntimeType {
        return RuntimeType.forInlineFun("DefaultResolver", ClientRustModule.Config.endpoint) {
            generateBddResolverImpl(this)
        }
    }

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

    private fun generateEndpoint(endpoint: Endpoint): Writable {
        val registry = FunctionRegistry(stdlib)
        val context = Context(registry, runtimeConfig)
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

    private fun generateBddResolverImpl(writer: RustWriter) {
        val nodes = generateNodeArray()
        val conditionCount = bddTrait.conditions.size
        val resultCount = bddTrait.results.size
        val typeGenerator = EndpointTypesGenerator.fromContext(codegenContext)
        // Create context for expression generation with stdlib
        val registry = FunctionRegistry(stdlib)
        val context = Context(registry, runtimeConfig)

        // Render conditions to a dummy writer to populate the function registry
        // This is the same trick used by EndpointResolverGenerator
        val dummyWriter = RustWriter.root()
        bddTrait.conditions.withIndex().forEach { (idx, cond) ->
            val bddExpressionGenerator =
                BddExpressionGenerator(cond, Ownership.Borrowed, context, allRefs, mutableSetOf())
            bddExpressionGenerator.generateCondition(cond, 9999999)(
                RustWriter.root(),
            )
        }

        // Now get the functions that were actually used during condition generation
        val fnsUsed = registry.fnsUsed()

        val endpointLib =
            InlineDependency.forRustFile(
                RustModule.pubCrate("bdd_interpreter"),
                "/inlineable/src/endpoint_lib/bdd_interpreter.rs",
                EndpointsLib.partitionResolver(runtimeConfig).dependency!!,
            )

        // Build the scope with condition evaluations
        val conditionScope =
            bddTrait.conditions.withIndex().associate { (idx, cond) ->
                val bddExpressionGenerator =
                    BddExpressionGenerator(cond, Ownership.Borrowed, context, allRefs, mutableSetOf())
                "cond_$idx" to bddExpressionGenerator.generateCondition(cond, idx)
            }

        // Identify which conditions produce results
        val conditionProducesResult = bddTrait.conditions.map { it.producesResult() }

        // Build additional args for custom runtime functions
        val additionalArgsSignature = fnsUsed.mapNotNull { it.additionalArgsSignatureBdd() }
        val additionalArgsInvocation = fnsUsed.mapNotNull { it.additionalArgsInvocation("self") }

        writer.rustTemplate(
            """
            use #{EndpointLib}::{evaluate_bdd, BddNode,};

            ##[derive(Debug)]
            /// The default endpoint resolver.
            pub struct DefaultResolver {
                #{custom_fields}
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
                        #{custom_fields_init}
                    }
                }

                ##[allow(clippy::needless_borrow)]
                pub(crate) fn evaluate_fn(
                    &self,
                ) -> impl for<'a> FnMut(
                    &ConditionFn,
                    &'a Params,
                    &mut ConditionContext<'a>,
                    &mut crate::endpoint_lib::diagnostic::DiagnosticCollector,
                ) -> bool
                       + '_ {
                    move |cond, params, context, _diagnostic_collector| {
                        cond.evaluate(
                            params,
                            context,
                            #{custom_fields_args}
                            _diagnostic_collector,
                        )
                    }
                }

                fn resolve_endpoint<'a>(&'a self, params: &'a #{Params}) -> #{Result}<#{SmithyEndpoint}, #{BoxError}> {
                    let mut diagnostic_collector = #{DiagnosticCollector}::new();
                    let mut condition_context = ConditionContext::default();
                    let result = evaluate_bdd(
                        NODES,
                        CONDITIONS,
                        RESULTS,
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

            ##[derive(Debug)]
            pub(crate) enum ConditionFn {
                ${(0 until conditionCount).joinToString(",\n    ") { "Cond$it" }}
            }

            impl ConditionFn {
                ##[allow(unused_variables, unused_parens, clippy::double_parens,
                    clippy::useless_conversion, clippy::bool_comparison, clippy::comparison_to_empty,
                    clippy::needless_borrow, clippy::useless_asref, )]
                fn evaluate<'a>(&self, params: &'a Params, context: &mut ConditionContext<'a>#{additional_args_sig_prefix}#{additional_args_sig}, _diagnostic_collector: &mut #{DiagnosticCollector}) -> bool {
                    // Param bindings
                    #{param_bindings:W}

                    // Non-Param references
                    #{non_param_ref_bindings:W}
                    match self {
                        ${(0 until conditionCount).joinToString(",\n") { idx -> "Self::Cond$idx => #{cond_$idx:W}" }}
                    }
                }
            }

            #{ConditionContext:W}

            const CONDITIONS: &[ConditionFn] = &[
                ${(0 until conditionCount).joinToString(",\n    ") { "ConditionFn::Cond$it" }}
            ];

            ##[derive(Debug, Clone)]
            enum ResultEndpoint {
                ${(0 until resultCount).joinToString(",\n    ") { "Result$it" }}
            }

            impl<'a> ResultEndpoint {
                ##[allow(unused_variables, clippy::useless_asref)]
                fn to_endpoint(&self, params: &'a Params, context: &ConditionContext<'a>) -> #{Result}<#{Endpoint}, #{ResolveEndpointError}> {
                    // Param bindings
                    #{param_bindings_for_results:W}

                    // Non-Param references
                    #{non_param_ref_bindings_for_results:W}

                    match self {
                        #{result_arms:W}
                    }
                }
            }

            const RESULTS: &[ResultEndpoint] = &[
                ${(0 until resultCount).joinToString(",\n    ") { "ResultEndpoint::Result$it" }}
            ];



            //TODO(BDD) move this to endpoint_lib
            /// Helper trait to implement the coalesce! macro
            pub trait Coalesce {
                /// The first arg
                type Arg1;
                /// The second arg
                type Arg2;
                /// The result of comparing Arg1 and Arg1
                type Result;

                /// Evaluates arguments in order and returns the first non-empty result, otherwise returns the result of the last argument.
                fn coalesce(&self) -> fn(Self::Arg1, Self::Arg2) -> Self::Result;
            }

            impl<T> Coalesce for &&&(&Option<T>, &Option<T>) {
                type Arg1 = Option<T>;
                type Arg2 = Option<T>;
                type Result = Option<T>;

                fn coalesce(&self) -> fn(Self::Arg1, Self::Arg2) -> Self::Result {
                    |a: Option<T>, b: Option<T>| a.or(b)
                }
            }

            impl<T> Coalesce for &&(&Option<T>, &T) {
                type Arg1 = Option<T>;
                type Arg2 = T;
                type Result = T;

                fn coalesce(&self) -> fn(Self::Arg1, Self::Arg2) -> Self::Result {
                    |a: Option<T>, b: T| a.unwrap_or(b)
                }
            }

            impl<T, U> Coalesce for &(&T, &U) {
                type Arg1 = T;
                type Arg2 = U;
                type Result = T;

                fn coalesce(&self) -> fn(Self::Arg1, Self::Arg2) -> Self::Result {
                    |a: T, _b| a
                }
            }

            /// Evaluates arguments in order and returns the first non-empty result, otherwise returns the result of the last argument.
            ##[macro_export]
            macro_rules! coalesce {
                (${"$"}a:expr) => {${"$"}a};
                (${"$"}a:expr, ${"$"}b:expr) => {{
                    use Coalesce;
                    let a = ${"$"}a;
                    let b = ${"$"}b;
                    (&&&(&a, &b)).coalesce()(a, b)
                }};
                (${"$"}a:expr, ${"$"}b:expr ${'$'}(, ${"$"}c:expr)* ${'$'}(,)?) => {
                    ${"$"}crate::coalesce!(${"$"}crate::coalesce!(${"$"}a, ${"$"}b) ${'$'}(, ${"$"}c)*)
                }
            }

            $nodes
            """,
            *preludeScope,
            "Endpoint" to Types(runtimeConfig).smithyEndpoint,
            "EndpointLib" to RuntimeType.forInlineDependency(endpointLib),
            "ServiceSpecificEndpointResolver" to codegenContext.serviceSpecificEndpointResolver(),
            "ResolveEndpointError" to Types(runtimeConfig).resolveEndpointError,
            *Types(runtimeConfig).toArray(),
            "Params" to typeGenerator.paramsStruct(),
            "ConditionContext" to generateContextStruct(),
            "param_bindings" to generateParamBindings(),
            "param_bindings_for_results" to generateParamBindingsForResults(),
            "non_param_ref_bindings" to generateNonParamReferences(),
            "non_param_ref_bindings_for_results" to generateNonParamReferencesForResult(),
            *conditionScope.toList().toTypedArray(),
            "DiagnosticCollector" to EndpointsLib.DiagnosticCollector,
            "PartitionResolver" to EndpointsLib.partitionResolver(runtimeConfig),
            "custom_fields" to writable { fnsUsed.mapNotNull { it.structFieldBdd() }.forEach { rust("#W,", it) } },
            "custom_fields_init" to
                writable {
                    fnsUsed.mapNotNull { it.structFieldInitBdd() }.forEach { rust("#W,", it) }
                },
            "custom_fields_args" to
                writable {
                    fnsUsed.mapNotNull { it.additionalArgsInvocation("self") }.forEach { rust("#W,", it) }
                },
            "additional_args_sig_prefix" to writable { if (additionalArgsSignature.isNotEmpty()) rust(", ") },
            "additional_args_sig" to
                writable {
                    additionalArgsSignature.forEachIndexed { i, it ->
                        if (i > 0) rust(", ")
                        rust("#W", it)
                    }
                },
            "additional_args_invoke_prefix" to writable { if (additionalArgsInvocation.isNotEmpty()) rust(", ") },
            "additional_args_invoke" to
                writable {
                    additionalArgsInvocation.forEachIndexed { i, it ->
                        if (i > 0) rust(", ")
                        rust("#W", it)
                    }
                },
            "result_arms" to generateResultArms(context),
            "BoxError" to RuntimeType.boxError(runtimeConfig),
            "SmithyEndpoint" to Types(runtimeConfig).smithyEndpoint,
        )
    }

    private fun generateParamBindings() =
        writable {
            bddTrait.parameters.toList().forEach {
                rust("let ${it.memberName()} = &params.${it.memberName()};")
            }
        }

    private fun generateParamBindingsForResults() =
        writable {
            bddTrait.parameters.toList().forEach {
                if (it.isRequired) {
                    if (it.type == ParameterType.STRING || it.type == ParameterType.STRING_ARRAY) {
                        rust("let ${it.memberName()} = &params.${it.memberName()};")
                    } else {
                        rust("let ${it.memberName()} = params.${it.memberName()};")
                    }
                } else {
                    val stringRefs =
                        allRefs.filter { ref -> ref.rustType == AnnotatedRefs.RustType.String }.map { ref -> ref.name }

                    if (stringRefs.contains(it.memberName())) {
                        rust("let ${it.memberName()} = params.${it.memberName()}.as_ref().map(|s| s.clone()).unwrap_or_default();")
                    } else {
                        rust("let ${it.memberName()} = params.${it.memberName()}.unwrap_or_default();")
                    }
                }
            }
        }

    private fun generateResultArms(context: Context) =
        writable {
            val visitor = RuleVisitor(context)
            val results = bddTrait.results
            bddTrait.results.forEachIndexed { idx, rule ->
                // Skip NoMatchRule (index 0) - it's a sentinel that doesn't support visitor pattern
                if (rule is NoMatchRule) {
                    rustTemplate(
                        "Self::Result$idx => #{Err}(#{ResolveEndpointError}::message(\"No endpoint rule matched\")),\n",
                        *preludeScope,
                        "ResolveEndpointError" to Types(runtimeConfig).resolveEndpointError,
                    )
                } else {
                    val ruleOutput = rule.accept(visitor)
                    rustTemplate("Self::Result$idx => #{output:W},\n", "output" to ruleOutput)
                }
            }
        }

    // All non-param references start as None
    private fun generateNonParamReferences() =
        writable {
            val varRefs = allRefs.variableRefs()
            varRefs.forEach {
                rust("let ${it.value.name} = &mut context.${it.value.name};")
            }
        }

    // All non-param references are Option<>. When the BDD is compiled Smithy guarantees that
    // the refs used to construct the result are Some, so it is same to unwrap_or_default them
    // all and just use the necessary ones.
    private fun generateNonParamReferencesForResult() =
        writable {
            val varRefs = allRefs.variableRefs()
            varRefs.values.forEachIndexed { idx, it ->
                val rustName = it.name
                if (allRefs.filter { ref -> ref.rustType == AnnotatedRefs.RustType.Document }
                        .map { ref -> ref.name }.contains(rustName)
                ) {
                    rust(
                        """
                        let binding_$idx = context.$rustName.as_ref().map(|s| s.clone()).unwrap_or_default();
                        let $rustName = binding_$idx.as_string().unwrap_or_default();
                        """.trimIndent(),
                    )
                } else {
                    rust("let $rustName = context.$rustName.as_ref().map(|s| s.clone()).unwrap_or_default();")
                }
            }
        }

    private fun generateContextStruct() =
        writable {
            val varRefs = allRefs.variableRefs()

            val memberDefs =
                varRefs.map {
                    val value = it.value
                    val fnName = it.value.condition?.function?.name?.toString()
                    val fn = it.value.condition?.function
                    val type =
                        if (it.value.type is OptionalType) {
                            (it.value.type as OptionalType).inner()
                        } else {
                            it.value.type
                        }
                    // TODO(BDD) I should maybe use the fnRegistry for this?
                    val rustType =
                        when {
                            type is StringType -> "String"
                            type is BooleanType -> "bool"
                            type is ArrayType -> "Vec<String>"
                            fn is ParseUrl -> "crate::endpoint_lib::parse_url::Url<'a>"
                            fn is GetAttr -> "aws_smithy_types::Document"
                            fnName == "aws.partition" -> "crate::endpoint_lib::partition::Partition<'a>"
                            fnName == "aws.parseArn" -> "crate::endpoint_lib::arn::Arn<'a>"
                            else -> throw IllegalArgumentException("Unsupported reference type $it")
                        }
                    "pub(crate) ${it.value.name}: Option<$rustType>"
                }.joinToString(",\n", postfix = ",\n")

            rustTemplate(
                """
                // These are all optional since they are set by condition and will
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

    private fun generateNodeArray(): String {
        val bdd = bddTrait.bdd
        val nodes = mutableListOf<String>()

        bdd.getNodes { var_, high, low ->
            nodes.add("BddNode { condition_index: $var_, high_ref: $high, low_ref: $low }")
        }

        return "const NODES: &[BddNode] = &[\n    ${nodes.joinToString(",\n    ")}\n];"
    }
}

/**
 * Determines if a condition produces a result that should be stored in the context.
 */
private fun Condition.producesResult(): Boolean = this.result.isPresent

/**
 * Container for annotated references with lookup by name.
 */
class AnnotatedRefs(
    private val refs: Map<String, AnnotatedRef>,
) {
    enum class RefType {
        Parameter,
        Variable,
    }

    enum class RustType {
        Document,
        String,
        StringArray,
        Bool,
        Arn,
        Partition,
        Url,
    }

    data class AnnotatedRef(
        val name: String,
        val refType: RefType,
        val isOptional: Boolean,
        val rustType: RustType,
        // These two are only present when RefType == Variable
        val condition: Condition?,
        val type: Type?,
    )

    operator fun get(name: String): AnnotatedRef? = refs[name]

    fun filter(predicate: (AnnotatedRef) -> Boolean): List<AnnotatedRef> = refs.values.filter(predicate)

    fun map(transform: (AnnotatedRef) -> String): List<String> = refs.values.map(transform)

    fun variableRefs(): Map<String, AnnotatedRef> = refs.filter { entry -> entry.value.refType == RefType.Variable }

    fun paramRefs(): Map<String, AnnotatedRef> = refs.filter { entry -> entry.value.refType == RefType.Parameter }

    companion object {
        fun from(bddTrait: EndpointBddTrait): AnnotatedRefs {
            val refs = mutableMapOf<String, AnnotatedRef>()

            bddTrait.parameters.forEach { param ->
                val rustType =
                    when (param.type) {
                        ParameterType.STRING -> RustType.String
                        ParameterType.STRING_ARRAY -> RustType.StringArray
                        ParameterType.BOOLEAN -> RustType.Bool
                        null -> RustType.String
                    }
                refs[param.memberName()] =
                    AnnotatedRef(param.memberName(), RefType.Parameter, !param.isRequired, rustType, null, null)
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
                    val rustType =
                        when {
                            returnType is AnyType -> RustType.Document
                            returnType is StringType -> RustType.String
                            returnType is BooleanType -> RustType.Bool
                            returnType is ArrayType ->
                                when (returnType.member) {
                                    is StringType -> RustType.StringArray
                                    else -> throw IllegalArgumentException("Unsupported reference type $returnType")
                                }

                            cond.function is ParseUrl -> RustType.Url
                            cond.function.name == "aws.partition" -> RustType.Partition
                            cond.function.name == "aws.parseArn" -> RustType.Arn
                            else -> throw IllegalArgumentException("Unsupported reference type $returnType")
                        }
                    refs[result.rustName()] =
                        AnnotatedRef(result.rustName(), RefType.Variable, true, rustType, cond, returnType)
                }
            }

            return AnnotatedRefs(refs)
        }
    }
}
