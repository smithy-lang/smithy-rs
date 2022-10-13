/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoints

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.KnowledgeIndex
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rulesengine.language.EndpointRuleSet
import software.amazon.smithy.rulesengine.language.model.Partition
import software.amazon.smithy.rulesengine.language.stdlib.partition.DefaultPartitionDataProvider
import software.amazon.smithy.rulesengine.language.syntax.Identifier
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameter
import software.amazon.smithy.rulesengine.language.syntax.parameters.ParameterType
import software.amazon.smithy.rulesengine.language.syntax.rule.Condition
import software.amazon.smithy.rulesengine.traits.ContextParamTrait
import software.amazon.smithy.rulesengine.traits.EndpointRuleSetTrait
import software.amazon.smithy.rulesengine.traits.EndpointTestCase
import software.amazon.smithy.rulesengine.traits.EndpointTestsTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.makeOptional
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.toRustName

class EndpointsIndex(model: Model) : KnowledgeIndex {

    private val rulesets: HashMap<ServiceShape, EndpointRuleSet?> = HashMap()
    private val tests: HashMap<ServiceShape, List<EndpointTestCase>?> = HashMap()

    fun rulesetForService(serviceShape: ServiceShape) = rulesets.computeIfAbsent(
        serviceShape,
    ) {
        serviceShape.getTrait<EndpointRuleSetTrait>()?.ruleSet?.let {
            // Create the result, cache the `.typecheck()` result for the ruleset, then return the ruleset
            // `.typecheck()` must be called before we can check the `.type()` of anything within the ruleset.
            EndpointRuleSet.fromNode(it).also { ruleset -> ruleset.typecheck() }
        }
    }

    fun testCasesForService(serviceShape: ServiceShape) = tests.computeIfAbsent(
        serviceShape,
    ) { serviceShape.getTrait<EndpointTestsTrait>()?.testCases }

    val partitions: List<Partition> by lazy {
        DefaultPartitionDataProvider().loadPartitions().partitions()
    }

    companion object {
        fun of(model: Model): EndpointsIndex {
            return model.getKnowledge(EndpointsIndex::class.java) { EndpointsIndex(it) }
        }
    }
}

/**
 * Utility function to convert an [Identifier] into a valid Rust identifier (snake case)
 */
internal fun Identifier.toRustName(): String = this.toString().toRustName()

internal fun Condition.toRustName(): String? = this.result.orNull()?.toRustName()

/**
 * Returns the memberName() for a given [Parameter]
 */
fun Parameter.memberName(): String {
    return name.toRustName()
}

fun ContextParamTrait.memberName(): String = this.name.toRustName()

/**
 * Returns the symbol for a given parameter. This enables [RustWriter] to generate the correct [RustType].
 */
fun Parameter.symbol(): Symbol {
    val rustType = when (this.type) {
        ParameterType.STRING -> RustType.String
        ParameterType.BOOLEAN -> RustType.Bool
        else -> TODO("unexpected type: ${this.type}")
    }
    // Parameter return types are always optional
    return Symbol.builder().rustType(rustType).build().letIf(!this.isRequired) { it.makeOptional() }
}
