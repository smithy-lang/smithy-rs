/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.util

fun PANIC(reason: String): Nothing = throw NotImplementedError(reason)
fun UNREACHABLE(reason: String): Nothing = throw IllegalStateException("This should be unreachable: $reason")
