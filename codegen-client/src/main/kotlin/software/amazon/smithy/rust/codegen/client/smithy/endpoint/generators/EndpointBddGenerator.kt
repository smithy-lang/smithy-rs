/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators

import software.amazon.smithy.rulesengine.language.evaluation.type.AnyType
import software.amazon.smithy.rulesengine.language.evaluation.type.ArrayType
import software.amazon.smithy.rulesengine.language.evaluation.type.BooleanType
import software.amazon.smithy.rulesengine.language.evaluation.type.OptionalType
import software.amazon.smithy.rulesengine.language.evaluation.type.StringType
import software.amazon.smithy.rulesengine.language.syntax.expressions.functions.GetAttr
import software.amazon.smithy.rulesengine.language.syntax.expressions.functions.ParseUrl
import software.amazon.smithy.rulesengine.language.syntax.parameters.ParameterType
import software.amazon.smithy.rulesengine.language.syntax.rule.Condition
import software.amazon.smithy.rulesengine.traits.EndpointBddTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.Context
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointTypesGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointsLib
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.Types
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.memberName
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen.BddExpressionGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen.Ownership
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rustName
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
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

    fun generateBddResolver(): RuntimeType {
        return RuntimeType.forInlineFun("DefaultResolver", ClientRustModule.Config.endpoint) {
            generateBddResolverImpl(this)
        }
    }

    private fun generateBddResolverImpl(writer: RustWriter) {
        val nodes = generateNodeArray()
        val conditionCount = bddTrait.conditions.size
        val resultCount = bddTrait.results.size
        val typeGenerator = EndpointTypesGenerator.fromContext(codegenContext)
        val conditionGenerator = ConditionEvaluationGenerator(codegenContext, stdlib, bddTrait)
        // Create context for expression generation with stdlib
        val registry = FunctionRegistry(stdlib)
        val context = Context(registry, runtimeConfig)

        // Render conditions to a dummy writer to populate the function registry
        // This is the same trick used by EndpointResolverGenerator
        val dummyWriter = RustWriter.root()
        bddTrait.conditions.withIndex().forEach { (idx, cond) ->
            val bddExpressionGenerator =
                BddExpressionGenerator(cond, Ownership.Borrowed, context, listAllRefs(), mutableSetOf())
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
                    BddExpressionGenerator(cond, Ownership.Borrowed, context, listAllRefs(), mutableSetOf())
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
            use #{ResolveEndpointError};

            ##[derive(Debug)]
            pub struct DefaultResolver {
                #{custom_fields}
            }

            impl  DefaultResolver {
                pub fn new() -> Self {
                    Self {
                        #{custom_fields_init}
                    }
                }

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
                            self.partition_resolver,
                            _diagnostic_collector,
                        )
                    }
                }
            }

            impl #{ServiceSpecificEndpointResolver} for DefaultResolver {
                fn resolve_endpoint<'a>(&'a self, params: &'a #{Params}) -> #{EndpointFuture}<'a> {
                    let mut diagnostic_collector = #{DiagnosticCollector}::new();
                    let mut condition_context = ConditionContext::default();
                    let result = evaluate_bdd(
                        &NODES,
                        &CONDITIONS,
                        &RESULTS,
                        (${bddTrait.bdd.rootRef} - 1),
                        params,
                        &mut condition_context,
                        &mut diagnostic_collector,
                        self.evaluate_fn(),
                    );

                    #{EndpointFuture}::ready(match result {
                        #{Some}(endpoint) => match endpoint.to_endpoint() {
                            Ok(ep) => Ok(ep),
                            Err(err) => Err(Box::new(err)),
                        },
                        #{None} => #{Err}(Box::new(#{ResolveEndpointError}::message("No endpoint rule matched"))),
                    })
                }
            }

            ##[derive(Debug)]
            enum ConditionFn {
                ${(0 until conditionCount).joinToString(",\n    ") { "Cond$it" }}
            }

            impl ConditionFn {
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

            impl ResultEndpoint {
                fn to_endpoint(&self) -> #{Result}<#{Endpoint}, #{ResolveEndpointError}> {
                    match self {
                        ${
                (0 until resultCount).joinToString(",\n            ") { idx ->
                    """Self::Result$idx => Err(#{ResolveEndpointError}::message("TODO(bdd): result $idx"))"""
                }
            }
                    }
                }
            }

            const RESULTS: &[ResultEndpoint] = &[
                ${(0 until resultCount).joinToString(",\n    ") { "ResultEndpoint::Result$it" }}
            ];



            //TODO(BDD) move this to endpoint_lib
            pub trait Coalesce {
                type Arg1;
                type Arg2;
                type Result;

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
            "non_param_ref_bindings" to generateNonParamReferences(),
            *conditionScope.toList().toTypedArray(),
            "DiagnosticCollector" to EndpointsLib.DiagnosticCollector,
            "PartitionResolver" to EndpointsLib.partitionResolver(runtimeConfig),
            "custom_fields" to writable { fnsUsed.mapNotNull { it.structFieldBdd() }.forEach { rust("#W,", it) } },
            "custom_fields_init" to
                writable {
                    fnsUsed.mapNotNull { it.structFieldInitBdd() }.forEach { rust("#W,", it) }
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
        )
    }

    private fun generateParamBindings() =
        writable {
            bddTrait.parameters.toList().forEach {
                rust("let ${it.memberName()} = &params.${it.memberName()};")
            }
        }

    // All non-param references start as None
    private fun generateNonParamReferences() =
        writable {
            val refs = extractNonParamReferences()
            refs.forEach {
                rust("let mut ${it.first.rustName()} = &mut context.${it.first.rustName()};")
            }
        }

    private fun generateContextStruct() =
        writable {
            val refs = extractNonParamReferences()

            val memberDefs =
                refs.map {
                    val fnName = it.third.function.name?.toString()
                    val fn = it.third.function
                    val type =
                        if (it.second is OptionalType) {
                            (it.second as OptionalType).inner()
                        } else {
                            it.second
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
                    "pub(crate) ${it.first.rustName()}: Option<$rustType>"
                }.joinToString(",\n")

            rustTemplate(
                """
                // These are all optional since they are set by condition and will
                // all be unset when we start evaluation
                ##[derive(Default)]
                pub(crate) struct ConditionContext<'a> {
                    $memberDefs
                }
                """.trimIndent(),
            )
        }

    private fun extractNonParamReferences() =
        bddTrait.conditions.mapNotNull { cond ->
            val result = cond.result.orElse(null)
            val returnType = cond.function.functionDefinition.returnType

            result?.let { Triple(it, returnType, cond) }
        }

    private fun generateNodeArray(): String {
        val bdd = bddTrait.bdd
        val nodes = mutableListOf<String>()

        bdd.getNodes { var_, high, low ->
            nodes.add("BddNode { condition_index: $var_, high_ref: $high, low_ref: $low }")
        }

        return "const NODES: &[BddNode] = &[\n    ${nodes.joinToString(",\n    ")}\n];"
    }

    private fun listAllRefs(): List<AnnotatedRef> {
        val refs = mutableListOf<AnnotatedRef>()

        bddTrait.parameters.forEach { param ->
            val rustType =
                when (param.type) {
                    ParameterType.STRING -> RustType.String
                    ParameterType.STRING_ARRAY -> RustType.StringArray
                    ParameterType.BOOLEAN -> RustType.Bool
                    null -> RustType.String
                }
            refs.add(AnnotatedRef(param.memberName(), RefType.Parameter, !param.isRequired, rustType))
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
                refs.add(AnnotatedRef(result.rustName(), RefType.Variable, true, rustType))
            }

//            println("LNJ REFS: ${refs}")
        }

        return refs
    }
}

/**
 * Determines if a condition produces a result that should be stored in the context.
 */
private fun Condition.producesResult(): Boolean = this.result.isPresent

enum class RefType {
    Parameter,
    Variable,
}

// Limited set of Rust types that refs can be
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
)
