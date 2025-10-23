/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators

import software.amazon.smithy.rulesengine.traits.EndpointBddTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.Context
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointTypesGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointsLib
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.Types
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.memberName
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
        val generator = EndpointTypesGenerator.fromContext(codegenContext)

        // Create context for expression generation with stdlib
        val registry = FunctionRegistry(stdlib)
        val context = Context(registry, runtimeConfig)

        val endpointLib =
            InlineDependency.forRustFile(
                RustModule.pubCrate("bdd_interpreter"),
                "/inlineable/src/endpoint_lib/bdd_interpreter.rs",
            )

        // Build the scope with condition evaluations
        val conditionScope =
            bddTrait.conditions.withIndex().associate { (idx, condition) ->
                "cond_$idx" to ConditionEvaluationGenerator.generateConditionEvaluation(condition, context)
            }

        // Render conditions to a dummy writer to populate the function registry
        // This is the same trick used by EndpointResolverGenerator
        val dummyWriter = RustWriter.root()
        conditionScope.values.forEach { it(dummyWriter) }

        // Now get the functions that were actually used during condition generation
        val fnsUsed = registry.fnsUsed()

        // Build additional args for custom runtime functions
        val additionalArgsSignature = fnsUsed.mapNotNull { it.additionalArgsSignature() }
        val additionalArgsInvocation = fnsUsed.mapNotNull { it.additionalArgsInvocation("self") }

        writer.rustTemplate(
            """
            use #{EndpointLib}::{evaluate_bdd, BddNode};
            use #{ResolveEndpointError};

            $nodes

            ##[derive(Debug)]
            enum ConditionFn {
                ${(0 until conditionCount).joinToString(",\n    ") { "Cond$it" }}
            }

            impl ConditionFn {
                fn evaluate(&self, params: &Params#{additional_args_sig_prefix}#{additional_args_sig}, _diagnostic_collector: &mut #{DiagnosticCollector}) -> bool {
                    #{param_bindings:W}
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

            ##[derive(Debug)]
            pub struct DefaultResolver {
                #{custom_fields}
            }

            impl DefaultResolver {
                pub fn new() -> Self {
                    Self {
                        #{custom_fields_init}
                    }
                }
            }

            impl #{ServiceSpecificEndpointResolver} for DefaultResolver {
                fn resolve_endpoint<'a>(&'a self, params: &'a #{Params}) -> #{EndpointFuture}<'a> {
                    let mut diagnostic_collector = #{DiagnosticCollector}::new();
                    let result = evaluate_bdd(
                        &NODES,
                        ${bddTrait.bdd.rootRef},
                        params,
                        &CONDITIONS,
                        &RESULTS,
                        |service_params, condition| condition.evaluate(service_params#{additional_args_invoke_prefix}#{additional_args_invoke}, &mut diagnostic_collector),
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
            """,
            *preludeScope,
            "Endpoint" to Types(runtimeConfig).smithyEndpoint,
            "EndpointLib" to RuntimeType.forInlineDependency(endpointLib),
            "ServiceSpecificEndpointResolver" to codegenContext.serviceSpecificEndpointResolver(),
            "ResolveEndpointError" to Types(runtimeConfig).resolveEndpointError,
            *Types(runtimeConfig).toArray(),
            "Params" to generator.paramsStruct(),
            "param_bindings" to generateParamBindings(),
            *conditionScope.toList().toTypedArray(),
            "DiagnosticCollector" to EndpointsLib.DiagnosticCollector,
            "custom_fields" to writable { fnsUsed.mapNotNull { it.structField() }.forEach { rust("#W,", it) } },
            "custom_fields_init" to writable { fnsUsed.mapNotNull { it.structFieldInit() }.forEach { rust("#W,", it) } },
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

    private fun generateNodeArray(): String {
        val bdd = bddTrait.bdd
        val nodes = mutableListOf<String>()

        bdd.getNodes { var_, high, low ->
            nodes.add("BddNode { condition_index: $var_, high_ref: $high, low_ref: $low }")
        }

        return "const NODES: &[BddNode] = &[\n    ${nodes.joinToString(",\n    ")}\n];"
    }
}
