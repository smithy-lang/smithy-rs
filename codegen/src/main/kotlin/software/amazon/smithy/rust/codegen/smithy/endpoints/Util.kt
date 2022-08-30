/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.endpoints

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.rulesengine.language.lang.Identifier
import software.amazon.smithy.rulesengine.language.lang.parameters.Parameter
import software.amazon.smithy.rulesengine.language.lang.parameters.ParameterType
import software.amazon.smithy.rust.codegen.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.smithy.letIf
import software.amazon.smithy.rust.codegen.smithy.makeOptional
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.util.toSnakeCase

/**
 * Utility function to convert an [Identifier] into a valid Rust identifier (snake case)
 */
fun Identifier.rustName(): String {
    return RustReservedWords.escapeIfNeeded(this.toString().toSnakeCase())
}

/**
 * Returns the memberName() for a given [Parameter]
 */
fun Parameter.memberName(): String {
    return name.rustName()
}

/**
 * Returns the symbol for a given parameter. This enables [RustWriter] to generate the correct [RustType].
 */
fun Parameter.symbol(): Symbol {
    val rustType = when (this.type) {
        ParameterType.STRING -> RustType.String
        ParameterType.BOOLEAN -> RustType.Bool
        else -> TODO("unexpected type: ${this.type}")
    }
    // Parameter return types are always optional
    return Symbol.builder().rustType(rustType).build().letIf(!this.isRequired) { it.makeOptional() }
}
