/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.util

import kotlin.collections.Map

/**
 * Deep merges two maps, with the properties of `other` taking priority over the properties of `this`.
 */
fun Map<String, Any?>.deepMergeWith(other: Map<String, Any?>): Map<String, Any?> =
    deepMergeMaps(this, other)

@Suppress("UNCHECKED_CAST")
private fun deepMergeMaps(left: Map<String, Any?>, right: Map<String, Any?>): Map<String, Any?> {
    val result = mutableMapOf<String, Any?>()
    for (leftEntry in left.entries) {
        val rightValue = right[leftEntry.key]
        if (leftEntry.value is Map<*, *> && rightValue is Map<*, *>) {
            result[leftEntry.key] =
                deepMergeMaps(leftEntry.value as Map<String, Any?>, rightValue as Map<String, Any?>)
        } else if (rightValue != null) {
            result[leftEntry.key] = rightValue
        } else {
            result[leftEntry.key] = leftEntry.value
        }
    }
    for (rightEntry in right.entries) {
        if (!left.containsKey(rightEntry.key)) {
            result[rightEntry.key] = rightEntry.value
        }
    }
    return result
}
