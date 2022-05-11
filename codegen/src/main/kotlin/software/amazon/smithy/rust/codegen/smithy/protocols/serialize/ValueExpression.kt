/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.serialize

sealed class ValueExpression {
    abstract val name: String

    data class Reference(override val name: String) : ValueExpression()
    data class Value(override val name: String) : ValueExpression()

    fun asValue(): String = when (this) {
        is Reference -> "*$name"
        is Value -> name
    }

    fun asRef(): String = when (this) {
        is Reference -> name
        is Value -> "&$name"
    }
}
