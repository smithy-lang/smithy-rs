/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoints

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.KnowledgeIndex
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rulesengine.language.EndpointRuleset
import software.amazon.smithy.rulesengine.language.lang.Identifier
import software.amazon.smithy.rulesengine.language.lang.parameters.Parameter
import software.amazon.smithy.rulesengine.language.lang.parameters.ParameterType
import software.amazon.smithy.rulesengine.traits.ContextParamTrait
import software.amazon.smithy.rulesengine.traits.EndpointRuleSetTrait
import software.amazon.smithy.rust.codegen.client.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

class EndpointRulesetIndex(model: Model) : KnowledgeIndex {

    private val rulesets: HashMap<ServiceShape, EndpointRuleset?> = HashMap()

    fun endpointRulesForService(serviceShape: ServiceShape) = rulesets.computeIfAbsent(
        serviceShape,
    ) { serviceShape.getTrait<EndpointRuleSetTrait>()?.ruleSet?.let { EndpointRuleset.fromNode(it) } }

    companion object {
        fun of(model: Model): EndpointRulesetIndex {
            return model.getKnowledge(EndpointRulesetIndex::class.java) { EndpointRulesetIndex(it) }
        }
    }
}

/**
 * Utility function to convert an [Identifier] into a valid Rust identifier (snake case)
 */
fun Identifier.rustName(): String {
    return this.toString().stringToRustName()
}

fun String.stringToRustName(): String = RustReservedWords.escapeIfNeeded(this.toSnakeCase())

/**
 * Returns the memberName() for a given [Parameter]
 */
fun Parameter.memberName(): String {
    return name.rustName()
}

fun ContextParamTrait.memberName(): String = this.name.stringToRustName()

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
