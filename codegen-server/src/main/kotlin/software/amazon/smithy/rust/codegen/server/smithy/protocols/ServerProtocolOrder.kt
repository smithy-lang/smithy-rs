/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.aws.traits.protocols.AwsJson1_1Trait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.protocol.traits.Rpcv2CborTrait
import software.amazon.smithy.rust.codegen.server.smithy.customize.ProtocolOrderConstraint
import java.util.PriorityQueue

/** Resolves the protocol detection order used by generated multi-protocol routers. */
internal object ServerProtocolOrder {
    private val defaultOrder =
        listOf(
            Rpcv2CborTrait.ID,
            AwsJson1_1Trait.ID,
            AwsJson1_0Trait.ID,
            RestJson1Trait.ID,
            RestXmlTrait.ID,
        )

    /**
     * Applies built-in ordering edges and decorator-contributed relative constraints.
     * Constraints apply only when both protocols are selected. Ties between currently available
     * protocols are broken by built-in order and then protocol shape ID.
     */
    fun resolve(
        protocolIds: List<ShapeId>,
        constraints: List<ProtocolOrderConstraint>,
    ): List<ShapeId> {
        val selectedProtocolIds = protocolIds.toSet()
        if (selectedProtocolIds.size != protocolIds.size) {
            val duplicateIds =
                protocolIds.groupingBy { it }
                    .eachCount()
                    .filterValues { it > 1 }
                    .keys
                    .sortedBy(ShapeId::toString)
            throw CodegenException("Duplicate server protocols cannot be ordered: $duplicateIds")
        }

        val selectedDefaultOrder = defaultOrder.filter { it in selectedProtocolIds }
        val baselineOrder =
            selectedDefaultOrder +
                selectedProtocolIds
                    .filterNot { it in defaultOrder }
                    .sortedBy(ShapeId::toString)
        val baselineIndex = baselineOrder.withIndex().associate { (index, protocol) -> protocol to index }

        val outgoing = selectedProtocolIds.associateWith { linkedSetOf<ShapeId>() }
        val incomingCount = selectedProtocolIds.associateWith { 0 }.toMutableMap()

        fun addEdge(
            before: ShapeId,
            after: ShapeId,
        ) {
            if (outgoing.getValue(before).add(after)) {
                incomingCount[after] = incomingCount.getValue(after) + 1
            }
        }

        selectedDefaultOrder.zipWithNext().forEach { (before, after) -> addEdge(before, after) }

        val activeConstraints =
            constraints.filter { constraint ->
                constraint.protocol in selectedProtocolIds && constraint.relativeTo in selectedProtocolIds
            }
        activeConstraints.forEach { constraint ->
            when (constraint) {
                is ProtocolOrderConstraint.Before -> addEdge(constraint.protocol, constraint.relativeTo)
                is ProtocolOrderConstraint.After -> addEdge(constraint.relativeTo, constraint.protocol)
            }
        }

        val available = PriorityQueue(compareBy<ShapeId> { baselineIndex.getValue(it) })
        incomingCount.filterValues { it == 0 }.keys.forEach(available::add)

        val orderedIds = mutableListOf<ShapeId>()
        while (available.isNotEmpty()) {
            val protocol = available.remove()
            orderedIds.add(protocol)
            outgoing.getValue(protocol).forEach { successor ->
                val remaining = incomingCount.getValue(successor) - 1
                incomingCount[successor] = remaining
                if (remaining == 0) {
                    available.add(successor)
                }
            }
        }

        if (orderedIds.size != protocolIds.size) {
            val cycleProtocols =
                incomingCount
                    .filterValues { it > 0 }
                    .keys
                    .sortedBy { baselineIndex.getValue(it) }
            val renderedConstraints = activeConstraints.joinToString().ifEmpty { "none" }
            throw CodegenException(
                "Protocol ordering constraints contain a cycle involving $cycleProtocols. " +
                    "Active decorator constraints: $renderedConstraints. Built-in order: $selectedDefaultOrder",
            )
        }

        return orderedIds
    }
}
