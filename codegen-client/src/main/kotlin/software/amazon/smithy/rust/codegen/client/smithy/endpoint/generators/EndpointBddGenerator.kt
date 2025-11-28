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
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
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
    private val allRefs = AnnotatedRefs.from(bddTrait)

    /**
     * Main entrypoint for creating a BDD based endpoint resolver
     */
    private fun generateBddResolverImpl(writer: RustWriter) {
        val nodes = generateNodeArray()
        val conditionCount = bddTrait.conditions.size
        val resultCount = bddTrait.results.size
        val typeGenerator = EndpointTypesGenerator.fromContext(codegenContext)
        // Create context for expression generation with stdlib
        val registry = FunctionRegistry(stdlib)
        val context = Context(registry, runtimeConfig)

        // Render conditions to a dummy writer to populate the function registry.
        // This is the same trick used by EndpointResolverGenerator.
        bddTrait.conditions.withIndex().forEach { (idx, cond) ->
            val bddExpressionGenerator =
                BddExpressionGenerator(cond, Ownership.Borrowed, context, allRefs, mutableSetOf())
            bddExpressionGenerator.generateCondition(cond, 9999999)(
                RustWriter.root(),
            )
        }

        // Now get the functions that were actually used during condition generation
        val fnsUsed = registry.fnsUsed()

        // Build additional args for custom runtime functions
        val additionalArgsSignature = fnsUsed.mapNotNull { it.additionalArgsSignatureBdd() }
        val additionalArgsInvocation = fnsUsed.mapNotNull { it.additionalArgsInvocation("self") }

        // Build the scope with condition evaluations
        val conditionScope =
            bddTrait.conditions.withIndex().associate { (idx, cond) ->
                val bddExpressionGenerator =
                    BddExpressionGenerator(cond, Ownership.Borrowed, context, allRefs, mutableSetOf())
                "cond_$idx" to bddExpressionGenerator.generateCondition(cond, idx)
            }

        writer.rustTemplate(
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
                fn evaluate<'a>(&self, params: &'a Params, context: &mut ConditionContext<'a>#{AdditionalArgsSigPrefix}#{AdditionalArgsSig}, _diagnostic_collector: &mut #{DiagnosticCollector}) -> bool {
                    // Param bindings
                    #{ParamBindings:W}

                    // Non-Param references
                    #{NonParamRefBindings:W}
                    match self {
                        ${(0 until conditionCount).joinToString(",\n") { idx -> "Self::Cond$idx => #{cond_$idx:W}" }}
                    }
                }
            }

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
                    #{ParamBindingsForResults:W}

                    // Non-Param references
                    #{NonParamRefBindingsForResults:W}

                    match self {
                        #{ResultArms:W}
                    }
                }
            }

            const RESULTS: &[ResultEndpoint] = &[
                ${(0 until resultCount).joinToString(",\n    ") { "ResultEndpoint::Result$it" }}
            ];

            #{Nodes:W}

            #{ConditionContext:W}
            """,
            *preludeScope,
            "AdditionalArgsSig" to
                writable {
                    additionalArgsSignature.forEachIndexed { i, it ->
                        if (i > 0) rust(", ")
                        rust("#W", it)
                    }
                },
            "AdditionalArgsSigPrefix" to writable { if (additionalArgsSignature.isNotEmpty()) rust(", ") },
            "BddNode" to EndpointsLib.bddNode,
            "BoxError" to RuntimeType.boxError(runtimeConfig),
            "ConditionContext" to generateContextStruct(),
            *conditionScope.toList().toTypedArray(),
            "CustomFields" to writable { fnsUsed.mapNotNull { it.structFieldBdd() }.forEach { rust("#W,", it) } },
            "CustomFieldsArgs" to
                writable {
                    fnsUsed.mapNotNull { it.additionalArgsInvocation("self") }.forEach { rust("#W,", it) }
                },
            "CustomFieldsInit" to
                writable {
                    fnsUsed.mapNotNull { it.structFieldInitBdd() }.forEach { rust("#W,", it) }
                },
            "DiagnosticCollector" to EndpointsLib.DiagnosticCollector,
            "Endpoint" to Types(runtimeConfig).smithyEndpoint,
            "EvaluateBdd" to EndpointsLib.evaluateBdd,
            "Nodes" to nodes,
            "NonParamRefBindings" to generateNonParamReferences(),
            "NonParamRefBindingsForResults" to generateNonParamReferencesForResult(),
            "ParamBindings" to generateParamBindings(),
            "ParamBindingsForResults" to generateParamBindingsForResults(),
            "Params" to typeGenerator.paramsStruct(),
            "ResolveEndpointError" to Types(runtimeConfig).resolveEndpointError,
            "ResultArms" to generateResultArms(context),
            "ServiceSpecificEndpointResolver" to codegenContext.serviceSpecificEndpointResolver(),
            "SmithyEndpoint" to Types(runtimeConfig).smithyEndpoint,
            *Types(runtimeConfig).toArray(),
//            "PartitionResolver" to EndpointsLib.partitionResolver(runtimeConfig),
//            "AdditionalArgsInvokePrefix" to writable { if (additionalArgsInvocation.isNotEmpty()) rust(", ") },
//            "additional_args_invoke" to
//                writable {
//                    additionalArgsInvocation.forEachIndexed { i, it ->
//                        if (i > 0) rust(", ")
//                        rust("#W", it)
//                    }
//                },
        )
    }

    /**
     * Generates bindings for the Params attached to the `endpointBddTrait`
     */
    private fun generateParamBindings() =
        writable {
            bddTrait.parameters.toList().forEach {
                rust("let ${it.memberName()} = &params.${it.memberName()};")
            }
        }

    /**
     * Generates bindings for the Params attached to the `endpointBddTrait` as they
     * are used to generate `Result`s. This requires some cloning and unwrapping. The
     * unwraps are all `unwrap_or_default`. This is safe since it will not panic, and
     * thanks to the guarantees of BDD compilation we know that if a value is used to
     * construct a Result it has a value, so default values will never be used.
     */
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
     * All non-param references are Option<>. When the BDD is compiled Smithy guarantees that
     * the refs used to construct the result are Some, so it is safe to unwrap_or_default them
     * all and just use the necessary ones.
     */
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

    /**
     * Generate the arms of the `Result` match statement.
     */
    private fun generateResultArms(context: Context) =
        writable {
            val visitor = RuleVisitor(context)
            bddTrait.results.forEachIndexed { idx, rule ->
                // Skip NoMatchRule (index 0) - it doesn't support visitor pattern
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
     * used in Condition evalutaion.
     */
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

    /**
     * Generate the `ConditionContext` struct that is passed to each condition evaluation.
     * This allows passing mutable state between the conditions.
     */
    private fun generateContextStruct() =
        writable {
            val varRefs = allRefs.variableRefs()
            val registry = FunctionRegistry(stdlib)

            val memberDefs =
                varRefs.map {
                    val fn = it.value.condition?.function!!
                    val fnDef = fn.functionDefinition
                    val registeredFn = registry.fnFor(fnDef.id)
                    val fnId = registeredFn?.id
                    val type =
                        if (it.value.type is OptionalType) {
                            (it.value.type as OptionalType).inner()
                        } else {
                            it.value.type
                        }
                    val rustType =
                        when {
                            // Simple types, doesn't matter what fn they come from
                            type is StringType -> RuntimeType.String
                            type is BooleanType -> RuntimeType.Bool
                            type is ArrayType -> RuntimeType.typedVec(RuntimeType.String)
                            // GetAttr is inlined, not a registered fn
                            fn is GetAttr -> RuntimeType.document(runtimeConfig)
                            // These types aren't easy to infer from the type of the reference.
                            // It is basically just an unnamed struct so we would have to match
                            // on the fields. Easier to key off of the function that sets it.
                            fnId == "parseURL" -> EndpointsLib.url()
                            fnId == "aws.partition" -> EndpointsLib.partition(runtimeConfig)
                            fnId == "aws.parseArn" -> EndpointsLib.arn()
                            else -> throw IllegalArgumentException("Unsupported reference type $it")
                        }
                    "pub(crate) ${it.value.name}: Option<${rustType.render()}>"
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

    /**
     * Generate the array of `BddNode`s that model the evaluation DAG
     */
    private fun generateNodeArray() =
        writable {
            val bdd = bddTrait.bdd
            val nodes = mutableListOf<String>()

            bdd.getNodes { var_, high, low ->
                nodes.add("#{BddNode} { condition_index: $var_, high_ref: $high, low_ref: $low }")
            }

            rustTemplate(
                "const NODES: &[#{BddNode}] = &[\n${nodes.joinToString(",\n")}\n];",
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
 * Container for annotated references, these are the variables the condition evaluation can potentially
 * refer to. They come in two variants: Parameters (which are immutable), and Variables (which are all Optional
 * and begin as None and might be set by a condition later during evaluation).
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
                        null -> throw IllegalArgumentException("Unsupported parameter type ${param.type}")
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
