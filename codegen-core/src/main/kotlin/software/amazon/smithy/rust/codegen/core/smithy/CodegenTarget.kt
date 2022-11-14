/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy

/**
 * Code generation mode: In some situations, codegen has different behavior for client vs. server (eg. required fields)
 */
enum class CodegenTarget {
    CLIENT, SERVER;

    /**
     * Convenience method to execute thunk if the target is for CLIENT
     */
    fun <B> ifClient(thunk: () -> B): B? = if (this == CLIENT) {
        thunk()
    } else {
        null
    }

    /**
     * Convenience method to execute thunk if the target is for SERVER
     */
    fun <B> ifServer(thunk: () -> B): B? = if (this == SERVER) {
        thunk()
    } else {
        null
    }
}
