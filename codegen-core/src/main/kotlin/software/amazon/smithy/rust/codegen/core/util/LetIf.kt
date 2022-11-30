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
    } else this
}
