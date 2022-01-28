/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators.client

import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeType

data class ClientGenerics(
    val connectorDefault: RuntimeType?,
    val middlewareDefault: RuntimeType?,
    val retryDefault: RuntimeType?,
    val client: RuntimeType
) {
    /** Declaration with defaults set */
    val decl = writable {
        rustTemplate(
            "<C #{c:W}, M#{m:W}, R#{r:W}>",
            "c" to defaultType(connectorDefault),
            "m" to defaultType(middlewareDefault),
            "r" to defaultType(retryDefault)
        )
    }

    /** Instantiation */
    val inst: String = "<C, M, R>"

    /** Trait bounds */
    val bounds = writable {
        rustTemplate(
            """
            C: #{client}::bounds::SmithyConnector,
            M: #{client}::bounds::SmithyMiddleware<C>,
            R: #{client}::retry::NewRequestPolicy,
            """,
            "client" to client
        )
    }

    private fun defaultType(default: RuntimeType?) = writable {
        default?.also { rust("= #T", default) }
    }
}
