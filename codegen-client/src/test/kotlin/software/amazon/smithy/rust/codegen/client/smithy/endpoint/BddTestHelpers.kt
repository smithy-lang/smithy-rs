/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.Base64

/**
 * Helpers for assembling minimal `@endpointBdd` traits inline in codegen tests.
 *
 * The BDD trait's `nodes` field is a base64-encoded sequence of signed 32-bit
 * **big-endian** integer triples: `[conditionIndex, highRef, lowRef]`. A reference
 * of `1` or `-1` is the NoMatch terminal; `>=100_000_000` encodes a result at
 * `results[ref - 100_000_000]`. The first node (index 0) **must** be the
 * canonical terminal `[-1, 1, -1]`.
 *
 * See `.kiro/bdd-sep.md` for the full encoding spec.
 */
internal object BddTestHelpers {
    /** Triple of (conditionIndex, highRef, lowRef) for one BDD node. */
    data class Node(val conditionIndex: Int, val high: Int, val low: Int)

    /** The canonical terminal node that must occupy index 0 of every BDD. */
    val TERMINAL = Node(-1, 1, -1)

    /** Result reference for `results[index]`. Index 0 is the implicit NoMatchRule. */
    fun resultRef(index: Int): Int = 100_000_000 + index

    /**
     * Encodes a list of nodes as the base64 string the BDD trait expects.
     * The first node is automatically the terminal — callers pass only the
     * non-terminal nodes.
     */
    fun encodeNodes(nodesAfterTerminal: List<Node>): String {
        val all = listOf(TERMINAL) + nodesAfterTerminal
        val buf = ByteBuffer.allocate(all.size * 12).order(ByteOrder.BIG_ENDIAN)
        all.forEach { n ->
            buf.putInt(n.conditionIndex)
            buf.putInt(n.high)
            buf.putInt(n.low)
        }
        return Base64.getEncoder().encodeToString(buf.array())
    }
}
