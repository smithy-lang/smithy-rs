/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.rust.codegen.rustlang.conditionalBlock
import software.amazon.smithy.rust.codegen.rustlang.rustInline
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeType

data class GenericTypeArg(
    val typeArg: String,
    val bound: RuntimeType? = null,
)

class GenericsGenerator(vararg genericTypeArgs: GenericTypeArg) {
    private val typeArgs: MutableList<GenericTypeArg>

    init {
        typeArgs = genericTypeArgs.toMutableList()
    }

    fun add(typeArg: GenericTypeArg) {
        typeArgs.add(typeArg)
    }

    fun declaration(withAngleBrackets: Boolean = true) = writable {
        // Write nothing if this generator is empty
        if (typeArgs.isNotEmpty()) {
            val typeArgs = typeArgs.joinToString(", ") { it.typeArg }

            conditionalBlock(
                "<",
                ">",
                conditional = withAngleBrackets,
            ) {
                rustInline(typeArgs)
            }
        }
    }

    fun bounds() = writable {
        // Only write bounds for generic type params with a bound
        typeArgs.map {
            val (typeArg, bound) = it

            if (bound != null) {
                rustTemplate("$typeArg: #{bound},\n", "bound" to bound)
            }
        }
    }

    fun parameters() = writable {
    }

    operator fun plus(operationGenerics: GenericsGenerator): GenericsGenerator {
        return GenericsGenerator(*listOf(typeArgs, operationGenerics.typeArgs).flatten().toTypedArray())
    }
}
