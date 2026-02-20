/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

import software.amazon.smithy.rulesengine.language.EndpointRuleSet
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameter
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameters
import software.amazon.smithy.rulesengine.traits.EndpointTestCase
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.EndpointBddGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.EndpointParamsGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.EndpointResolverGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.EndpointTestGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

/**
 * Entrypoint for Endpoints 2.0 Code generation
 *
 * This exposes [RuntimeType]s for the individual components of endpoints 2.0
 */
class EndpointTypesGenerator(
    private val codegenContext: ClientCodegenContext,
    private val rules: EndpointRuleSet?,
    val tests: List<EndpointTestCase>,
    private val bddParameters: Parameters?,
) {
    val params: Parameters = bddParameters ?: rules?.parameters ?: Parameters.builder().build()
    private val runtimeConfig = codegenContext.runtimeConfig
    private val customizations = codegenContext.rootDecorator.endpointCustomizations(codegenContext)
    private val stdlib =
        customizations
            .flatMap { it.customRuntimeFunctions(codegenContext) }
    private val endpointIndex = EndpointRulesetIndex.of(codegenContext.model)

    companion object {
        fun fromContext(codegenContext: ClientCodegenContext): EndpointTypesGenerator {
            val index = EndpointRulesetIndex.of(codegenContext.model)

            // If service has BDD trait, extract parameters from it
            // TODO(bdd): Make this the default when BDDs published for all models
            val bddTrait = index.getEndpointBddTrait(codegenContext.serviceShape)
            if (bddTrait != null) {
                return EndpointTypesGenerator(
                    codegenContext,
                    null,
                    index.endpointTests(codegenContext.serviceShape),
                    bddTrait.parameters,
                )
            }

            val rulesOrNull = index.endpointRulesForService(codegenContext.serviceShape)
            return EndpointTypesGenerator(
                codegenContext,
                rulesOrNull,
                index.endpointTests(codegenContext.serviceShape),
                null,
            )
        }
    }

    fun paramsStruct(): RuntimeType = EndpointParamsGenerator(codegenContext, params).paramsStruct()

    fun paramsBuilder(): RuntimeType = EndpointParamsGenerator(codegenContext, params).paramsBuilder()

    fun defaultResolver(): RuntimeType? =
        defaultResolverBdd() ?: rules?.let {
            EndpointResolverGenerator(codegenContext, stdlib).defaultEndpointResolver(
                it,
            )
        }

    fun defaultResolverBdd(): RuntimeType? =
        endpointIndex.getEndpointBddTrait(codegenContext.serviceShape)?.let {
            EndpointBddGenerator(
                codegenContext,
                it,
                stdlib,
            ).generateBddResolver()
        }

    fun testGenerator(): Writable =
        // BDD takes priority just like in EndpointDecorator
        // TODO(bdd): Make this the default when BDDs published for all models
        defaultResolverBdd()?.let {
            EndpointTestGenerator(
                tests,
                paramsStruct(),
                it,
                params,
                codegenContext = codegenContext,
            ).generate()
        } ?: defaultResolver()?.let {
            EndpointTestGenerator(
                tests,
                paramsStruct(),
                it,
                params,
                codegenContext = codegenContext,
            ).generate()
        }
            ?: {}

    /**
     * Load the builtIn value for [parameter] from the endpoint customizations. If the built-in comes from service config,
     * [config] refers to `&crate::config::Config`
     *
     * Exactly one endpoint customization must provide the value for this builtIn or null is returned.
     */
    fun builtInFor(
        parameter: Parameter,
        config: String,
    ): Writable? {
        val defaultProviders =
            customizations
                .mapNotNull { it.loadBuiltInFromServiceConfig(parameter, config) }
        if (defaultProviders.size > 1) {
            error("Multiple providers provided a value for the builtin $parameter")
        }
        return defaultProviders.firstOrNull()
    }
}
