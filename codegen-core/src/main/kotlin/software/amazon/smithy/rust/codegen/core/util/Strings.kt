/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.util

import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.utils.CaseUtils
import software.amazon.smithy.utils.StringUtils

fun String.doubleQuote(): String = StringUtils.escapeJavaString(this, "")

/**
 * Double quote a string, e.g. "abc" -> "\"abc\""
 */
fun String.dq(): String = this.doubleQuote()

// String extensions
fun String.toSnakeCase(): String {
    return CaseUtils.toSnakeCase(this)
}

fun String.toPascalCase(): String {
    return CaseUtils.toSnakeCase(this).let { CaseUtils.toPascalCase(it) }
}

fun String.toRustName(): String = RustReservedWords.escapeIfNeeded(this.toSnakeCase())
