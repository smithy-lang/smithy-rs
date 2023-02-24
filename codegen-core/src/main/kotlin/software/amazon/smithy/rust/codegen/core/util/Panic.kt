/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.util

/** Something has gone horribly wrong due to a coding error */
fun PANIC(reason: String = ""): Nothing = throw RuntimeException(reason)

/** This code should never be executed (but Kotlin cannot prove that) */
fun UNREACHABLE(reason: String): Nothing = throw IllegalStateException("This should be unreachable: $reason")
