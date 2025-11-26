/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.rulesengine.language.Endpoint
import software.amazon.smithy.rulesengine.language.EndpointRuleSet
import software.amazon.smithy.rulesengine.language.syntax.Identifier
import software.amazon.smithy.rulesengine.language.syntax.expressions.Expression
import software.amazon.smithy.rulesengine.language.syntax.expressions.literal.Literal
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameter
import software.amazon.smithy.rulesengine.language.syntax.parameters.ParameterType
import software.amazon.smithy.rulesengine.language.syntax.rule.Rule
import software.amazon.smithy.rulesengine.language.syntax.rule.RuleValueVisitor
import software.amazon.smithy.rulesengine.traits.ContextParamTrait
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.EndpointStdLib
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.FunctionRegistry
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.toType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.makeOptional
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.smithy.unsafeToRustName
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.core.util.orNull

data class Context(val functionRegistry: FunctionRegistry, val runtimeConfig: RuntimeConfig)

/**
 * Utility function to convert an [Identifier] into a valid Rust identifier (snake case)
 */
fun Identifier.rustName(): String {
    return this.toString().unsafeToRustName()
}

/**
 * Endpoints standard library
 */
object EndpointsLib {
    val DiagnosticCollector = endpointsLib("diagnostic").toType().resolve("DiagnosticCollector")

    fun partitionResolver(runtimeConfig: RuntimeConfig) =
        endpointsLib("partition", CargoDependency.smithyJson(runtimeConfig), CargoDependency.RegexLite).toType()
            .resolve("PartitionResolver")

    val substring =
        endpointsLib("substring", CargoDependency.Proptest).toType().resolve("substring")
    val isValidHostLabel =
        endpointsLib("host", CargoDependency.Proptest).toType().resolve("is_valid_host_label")
    val parseUrl =
        endpointsLib("parse_url", CargoDependency.Http, CargoDependency.Url)
            .toType()
            .resolve("parse_url")
    val uriEncode =
        endpointsLib("uri_encode", CargoDependency.PercentEncoding)
            .toType()
            .resolve("uri_encode")
    val coalesce =
        endpointsLib("coalesce").toType().resolve("coalesce!")
    val evaluateBdd =
        endpointsLib("bdd_interpreter").toType().resolve("evaluate_bdd")
    val bddNode =
        endpointsLib("bdd_interpreter").toType().resolve("BddNode")

    val awsParseArn = endpointsLib("arn").toType().resolve("parse_arn")
    val awsIsVirtualHostableS3Bucket =
        endpointsLib("s3", endpointsLib("host"), CargoDependency.RegexLite)
            .toType()
            .resolve("is_virtual_hostable_s3_bucket")

    private fun endpointsLib(
        name: String,
        vararg additionalDependency: RustDependency,
    ) = InlineDependency.forRustFile(
        RustModule.pubCrate(
            name,
            parent = EndpointStdLib,
        ),
        "/inlineable/src/endpoint_lib/$name.rs",
        *additionalDependency,
    )
}

class Types(runtimeConfig: RuntimeConfig) {
    private val smithyTypesEndpointModule =
        RuntimeType.smithyTypes(runtimeConfig).resolve("endpoint")
    val smithyHttpEndpointModule = RuntimeType.smithyHttp(runtimeConfig).resolve("endpoint")
    val smithyEndpoint = smithyTypesEndpointModule.resolve("Endpoint")
    val endpointFuture =
        RuntimeType.smithyRuntimeApiClient(runtimeConfig)
            .resolve("client::endpoint::EndpointFuture")
    private val endpointRtApi =
        RuntimeType.smithyRuntimeApiClient(runtimeConfig).resolve("client::endpoint")
    val resolveEndpointError = smithyHttpEndpointModule.resolve("ResolveEndpointError")

    fun toArray() =
        arrayOf(
            "Endpoint" to smithyEndpoint,
            "EndpointFuture" to endpointFuture,
            "SharedEndpointResolver" to endpointRtApi.resolve("SharedEndpointResolver"),
            "EndpointResolverParams" to endpointRtApi.resolve("EndpointResolverParams"),
            "ResolveEndpoint" to endpointRtApi.resolve("ResolveEndpoint"),
        )
}

/**
 * Returns the memberName() for a given [Parameter]
 */
fun Parameter.memberName(): String {
    return name.rustName()
}

fun ContextParamTrait.memberName(): String = this.name.unsafeToRustName()

/**
 * Returns the symbol for a given parameter. This enables [software.amazon.smithy.rust.codegen.core.rustlang.RustWriter] to generate the correct [RustType].
 */
fun Parameter.symbol(): Symbol {
    val rustType =
        when (this.type) {
            ParameterType.STRING -> RustType.String
            ParameterType.BOOLEAN -> RustType.Bool
            ParameterType.STRING_ARRAY -> RustType.Vec(RustType.String)
            else -> TODO("unexpected type: ${this.type}")
        }
    // Parameter return types are always optional
    return Symbol.builder().rustType(rustType).build().letIf(!this.isRequired) { it.makeOptional() }
}

/**
 * A class for fetching the set of auth schemes supported by an `EndpointRuleSet`.
 */
class AuthSchemeLister : RuleValueVisitor<Set<String>> {
    companion object {
        fun authSchemesForRuleset(endpointRuleSet: EndpointRuleSet): Set<String> {
            return AuthSchemeLister().visitTreeRule(endpointRuleSet.rules)
        }
    }

    override fun visitEndpointRule(endpoint: Endpoint): Set<String> {
        return endpoint.properties
            .getOrDefault(Identifier.of("authSchemes"), Literal.tupleLiteral(listOf()))
            .asTupleLiteral()
            .orNull()
            ?.let {
                it.map { authScheme ->
                    authScheme.asRecordLiteral().get()[Identifier.of("name")]!!
                        .asStringLiteral()
                        .get()
                        .expectLiteral()
                }
            }
            ?.toHashSet()
            ?: hashSetOf()
    }

    override fun visitTreeRule(rules: MutableList<Rule>): Set<String> {
        return rules.map { it.accept(this) }.reduce { a, b -> a.union(b) }
    }

    override fun visitErrorRule(error: Expression?): Set<String> {
        return setOf()
    }
}
