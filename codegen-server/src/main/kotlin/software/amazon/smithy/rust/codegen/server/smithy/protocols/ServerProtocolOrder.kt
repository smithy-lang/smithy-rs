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

/**
 * Resolves the protocol detection order used by generated multi-protocol routers.
 *
 * The algorithm first creates a deterministic baseline: selected built-in protocols follow [defaultOrder], while
 * additional protocols registered by downstream decorators follow in shape-ID order. It represents that built-in
 * order and active decorator-contributed `before`/`after` constraints as directed edges, then performs a topological
 * sort. When several protocols are available at the same time, their baseline position breaks the tie.
 *
 * Constraints mentioning an unselected protocol are ignored. Duplicate selections and cycles fail code generation.
 * Because decorators may register protocols and contribute relative constraints, the resolved order is extensible and
 * is not limited to the built-in protocols listed in this file.
 */
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

        // Kahn's topological sort tracks each protocol's outgoing edges and remaining incoming-edge count. A protocol
        // becomes available when its incoming count reaches zero.
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
            // Emitting this protocol satisfies one prerequisite for each successor. Decrement the successor's
            // incoming-edge count; when it reaches zero, all required predecessors have been emitted and the
            // successor can be added to the available queue.
            outgoing.getValue(protocol).forEach { successor ->
                val remaining = incomingCount.getValue(successor) - 1
                incomingCount[successor] = remaining
                if (remaining == 0) {
                    available.add(successor)
                }
            }
        }

        // If the topological result contains fewer protocols than the input graph, the available queue emptied while
        // some protocols still had incoming edges, which means the ordering constraints contain a cycle.
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
