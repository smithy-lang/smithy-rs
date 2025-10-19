/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators

import software.amazon.smithy.rulesengine.traits.EndpointBddTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.Types
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.util.toPascalCase

/**
 * Generator for BDD-based endpoint resolvers
 */
class EndpointBddGenerator(
    private val codegenContext: ClientCodegenContext,
    private val bddTrait: EndpointBddTrait,
) {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val serviceName = codegenContext.serviceShape.id.name.toPascalCase()
    private val moduleUseName = codegenContext.moduleUseName()

    fun generateBddResolver(): RuntimeType {
        return RuntimeType.forInlineFun("${serviceName}BddResolver", ClientRustModule.Config.endpoint) {
            generateBddResolverImpl(this)
        }
    }

    private fun generateBddResolverImpl(writer: RustWriter) {
        val nodes = generateNodeArray()
        val conditions = generateConditionArray()
        val results = generateResultArray()

        val endpointLib =
            InlineDependency.forRustFile(
                RustModule.pubCrate("bdd_interpreter"),
                "/inlineable/src/endpoint_lib/bdd_interpreter.rs",
            )

        writer.rustTemplate(
            """
            use #{EndpointLib}::{evaluate_bdd, BddNode};

            $nodes
            $conditions
            $results

            ##[derive(::std::fmt::Debug)]
            pub struct ${serviceName}BddResolver;

            impl ResolveEndpoint for ${serviceName}BddResolver {
                fn resolve_endpoint<'a>(&'a self, params: &'a Params) -> #{EndpointFuture}<'a> {
                    let result = evaluate_bdd(
                        &NODES,
                        ${bddTrait.bdd.rootRef},
                        params,
                        &CONDITIONS,
                        &RESULTS,
                        |_params, _condition| {
                            // TODO(bdd): Implement condition evaluation
                            false
                        }
                    );

                    match result {
                        #{Some}(endpoint) => #{EndpointFuture}::ready(#{Ok}(endpoint.clone())),
                        #{None} => #{EndpointFuture}::ready(#{Err}(#{Box}::new(#{ResolveEndpointError}::message("No endpoint rule matched")))),
                    }
                }
            }

            impl ${serviceName}BddResolver {
                pub fn new() -> Self {
                    Self
                }
            }
            """,
            *preludeScope,
            "SmithyRuntimeApi" to RuntimeType.smithyRuntimeApiClient(runtimeConfig),
            "EndpointLib" to RuntimeType.forInlineDependency(endpointLib),
            "SharedEndpointResolver" to
                RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("client::endpoint::SharedEndpointResolver"),
            "ResolveEndpointError" to
                RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("client::endpoint::error::ResolveEndpointError"),
            "EndpointFuture" to Types(runtimeConfig).endpointFuture,
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

    private fun generateConditionArray(): String {
        // TODO(bdd): Generate condition evaluation functions
        return "const CONDITIONS: &[fn(&Params) -> bool] = &[];"
    }

    private fun generateResultArray(): String {
        // TODO(bdd): Generate result endpoints
        return "const RESULTS: &[Endpoint] = &[];"
    }
}
