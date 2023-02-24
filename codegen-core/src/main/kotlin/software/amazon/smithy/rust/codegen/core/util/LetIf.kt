/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.core.util

/**
 * Utility function similar to `let` that conditionally applies [f] only if [cond] is true.
 */
fun <T> T.letIf(cond: Boolean, f: (T) -> T): T {
    return if (cond) {
        f(this)
    } else {
        this
    }
}

fun <T> List<T>.extendIf(condition: Boolean, f: () -> T) = if (condition) {
    this + listOf(f())
} else {
    this
}

fun <T> Boolean.thenSingletonListOf(f: () -> T): List<T> = if (this) {
    listOf(f())
} else {
    listOf()
}

/**
 * Returns this list if it is non-empty otherwise, it returns null
 */
fun<T> List<T>.orNullIfEmpty(): List<T>? = this.ifEmpty { null }
