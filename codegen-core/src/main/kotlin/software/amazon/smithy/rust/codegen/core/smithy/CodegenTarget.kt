/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy

/**
 * Code generation mode: In some situations, codegen has different behavior for client vs. server (eg. required fields)
 */
enum class CodegenTarget {
    CLIENT, SERVER
}

/**
 * Convenience extension to execute thunk if the target is for CodegenTarget.CLIENT
 */
fun <B> CodegenTarget.ifClient(thunk: () -> B): B? = if (this == CodegenTarget.CLIENT) {
    thunk()
} else {
    null
}

/**
 * Convenience extension to execute thunk if the target is for CodegenTarget.SERVER
 */
fun <B> CodegenTarget.ifServer(thunk: () -> B): B? = if (this == CodegenTarget.SERVER) {
    thunk()
} else {
    null
}
