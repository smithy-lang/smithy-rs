/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.client

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.rust.codegen.core.rustlang.GenericTypeArg
import software.amazon.smithy.rust.codegen.core.rustlang.RustGenerics
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

// TODO(enableNewSmithyRuntimeCleanup): Delete this client generics on/off switch headache
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
    fun sendBounds(operation: Symbol, operationOutput: Symbol, operationError: Symbol, retryClassifier: RuntimeType): Writable

    /** Convert this `FluentClientGenerics` into the more general `RustGenerics` */
    fun toRustGenerics(): RustGenerics

    /** bounds without where clause. If bounds does is not prefixed with `where\n`, then it gets the same value. **/
    val boundsWithoutWhereClause: Writable
}

class NoClientGenerics(private val runtimeConfig: RuntimeConfig) : FluentClientGenerics {
    /** Declaration with defaults set */
    override val decl = writable { }

    /** Instantiation of the Smithy client generics */
    override val smithyInst = writable {
        rustTemplate(
            "<#{DynConnector}, #{DynMiddleware}<#{DynConnector}>>",
            "DynConnector" to RuntimeType.smithyClient(runtimeConfig).resolve("erase::DynConnector"),
            "DynMiddleware" to RuntimeType.smithyClient(runtimeConfig).resolve("erase::DynMiddleware"),
        )
    }

    /** Instantiation */
    override val inst = ""

    /** Trait bounds */
    override val bounds = writable { }

    override val boundsWithoutWhereClause = writable {}

    /** Bounds for generated `send()` functions */
    override fun sendBounds(
        operation: Symbol,
        operationOutput: Symbol,
        operationError: Symbol,
        retryClassifier: RuntimeType,
    ): Writable =
        writable { }

    override fun toRustGenerics() = RustGenerics()
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
                #{bounds}
            """,
            "bounds" to boundsWithoutWhereClause,
        )
    }

    override val boundsWithoutWhereClause = writable {
        rustTemplate(
            """
            C: #{client}::bounds::SmithyConnector,
            M: #{client}::bounds::SmithyMiddleware<C>,
            R: #{client}::retry::NewRequestPolicy,
            """,
            "client" to client,
        )
    }

    /** Bounds for generated `send()` functions */
    override fun sendBounds(operation: Symbol, operationOutput: Symbol, operationError: Symbol, retryClassifier: RuntimeType): Writable = writable {
        rustTemplate(
            """
            where
            R::Policy: #{client}::bounds::SmithyRetryPolicy<
                #{Operation},
                #{OperationOutput},
                #{OperationError},
                #{RetryClassifier}
            >,
            """,
            "client" to client,
            "Operation" to operation,
            "OperationOutput" to operationOutput,
            "OperationError" to operationError,
            "RetryClassifier" to retryClassifier,
        )
    }

    override fun toRustGenerics(): RustGenerics = RustGenerics(
        GenericTypeArg("C", client.resolve("bounds::SmithyConnector")),
        GenericTypeArg("M", client.resolve("bounds::SmithyMiddleware<C>")),
        GenericTypeArg("R", client.resolve("retry::NewRequestPolicy")),
    )

    private fun defaultType(default: RuntimeType?) = writable {
        default?.also { rust("= #T", default) }
    }
}
