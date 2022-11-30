/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize

import software.amazon.smithy.rust.codegen.core.rustlang.autoDeref

sealed class ValueExpression {
    abstract val name: String

    data class Reference(override val name: String) : ValueExpression()
    data class Value(override val name: String) : ValueExpression()

    fun asValue(): String = when (this) {
        is Reference -> autoDeref(name)
        is Value -> name
    }

    fun asRef(): String = when (this) {
        is Reference -> name
        is Value -> "&$name"
    }

    override fun toString(): String = this.name
}
