/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.client

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.rust.codegen.client.rustlang.Writable
import software.amazon.smithy.rust.codegen.client.rustlang.rust
import software.amazon.smithy.rust.codegen.client.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.client.rustlang.writable
import software.amazon.smithy.rust.codegen.client.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.client.smithy.generators.GenericTypeArg
import software.amazon.smithy.rust.codegen.client.smithy.generators.GenericsGenerator

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
    fun sendBounds(input: Symbol, output: Symbol, error: RuntimeType, retryPolicy: Writable): Writable

    /** Convert this `FluentClientGenerics` into the more general `GenericsGenerator` */
    fun toGenericsGenerator(): GenericsGenerator
}

data class FlexibleClientGenerics(
    val connectorDefault: RuntimeType?,
    val middlewareDefault: RuntimeType?,
    val retryDefault: RuntimeType?,
    val client: RuntimeType,
) : FluentClientGenerics {
    /** Declaration with defaults set */
    override val decl = writable {
        rustTemplate(
            "<C#{c:W}, M#{m:W}, R#{r:W}>",
            "c" to defaultType(connectorDefault),
            "m" to defaultType(middlewareDefault),
            "r" to defaultType(retryDefault),
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
            "client" to client,
        )
    }

    /** Bounds for generated `send()` functions */
    override fun sendBounds(operation: Symbol, operationOutput: Symbol, operationError: RuntimeType, retryPolicy: Writable): Writable = writable {
        rustTemplate(
            """
            where
            R::Policy: #{client}::bounds::SmithyRetryPolicy<
                #{Operation},
                #{OperationOutput},
                #{OperationError},
                #{RetryPolicy:W}
            >
            """,
            "client" to client,
            "Operation" to operation,
            "OperationOutput" to operationOutput,
            "OperationError" to operationError,
            "RetryPolicy" to retryPolicy,
        )
    }

    override fun toGenericsGenerator(): GenericsGenerator = GenericsGenerator(
        GenericTypeArg("C", client.member("bounds::SmithyConnector")),
        GenericTypeArg("M", client.member("bounds::SmithyMiddleware<C>")),
        GenericTypeArg("R", client.member("retry::NewRequestPolicy")),
    )

    private fun defaultType(default: RuntimeType?) = writable {
        default?.also { rust("= #T", default) }
    }
}
