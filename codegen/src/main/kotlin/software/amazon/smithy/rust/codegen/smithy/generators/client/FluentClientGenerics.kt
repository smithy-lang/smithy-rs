/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.generators.client

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeType

interface FluentClientGenerics {
    /** Declaration with defaults set */
    val decl: Writable

    /** Instantiation of the Smithy client generics */
    val smithyInst: Writable

    /** Instantiation */
    val inst: String

    /** Bounds */
    val bounds: Writable

    /** Bounds for generated `send()` functions */
    fun sendBounds(input: Symbol, output: Symbol, error: RuntimeType): Writable
}

data class FlexibleClientGenerics(
    val connectorDefault: RuntimeType?,
    val middlewareDefault: RuntimeType?,
    val retryDefault: RuntimeType?,
    val client: RuntimeType
) : FluentClientGenerics {
    /** Declaration with defaults set */
    override val decl = writable {
        rustTemplate(
            "<C #{c:W}, M#{m:W}, R#{r:W}>",
            "c" to defaultType(connectorDefault),
            "m" to defaultType(middlewareDefault),
            "r" to defaultType(retryDefault)
        )
    }

    /** Instantiation of the Smithy client generics */
    override val smithyInst = writable { rust("<C, M, R>") }

    /** Instantiation */
    override val inst: String = "<C, M, R>"

    /** Trait bounds */
    override val bounds = writable {
        rustTemplate(
            """
            where
                C: #{client}::bounds::SmithyConnector,
                M: #{client}::bounds::SmithyMiddleware<C>,
                R: #{client}::retry::NewRequestPolicy,
            """,
            "client" to client
        )
    }

    /** Bounds for generated `send()` functions */
    override fun sendBounds(input: Symbol, output: Symbol, error: RuntimeType): Writable = writable {
        rustTemplate(
            """
            where
            R::Policy: #{client}::bounds::SmithyRetryPolicy<
                #{Input}OperationOutputAlias,
                #{Output},
                #{Error},
                #{Input}OperationRetryAlias
            >
            """,
            "client" to client,
            "Input" to input,
            "Output" to output,
            "Error" to error,
        )
    }

    private fun defaultType(default: RuntimeType?) = writable {
        default?.also { rust("= #T", default) }
    }
}
