/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.KnowledgeIndex
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rulesengine.language.EndpointRuleSet
import software.amazon.smithy.rulesengine.traits.EndpointRuleSetTrait
import software.amazon.smithy.rust.codegen.core.util.getTrait
import java.util.concurrent.ConcurrentHashMap

class EndpointRulesetIndex(model: Model) : KnowledgeIndex {

    private val rulesets: ConcurrentHashMap<ServiceShape, EndpointRuleSet?> = ConcurrentHashMap()

    fun endpointRulesForService(serviceShape: ServiceShape) = rulesets.computeIfAbsent(
        serviceShape,
    ) { serviceShape.getTrait<EndpointRuleSetTrait>()?.ruleSet?.let { EndpointRuleSet.fromNode(it) }?.also { it.typecheck() } }

    companion object {
        fun of(model: Model): EndpointRulesetIndex {
            return model.getKnowledge(EndpointRulesetIndex::class.java) { EndpointRulesetIndex(it) }
        }
    }
}
