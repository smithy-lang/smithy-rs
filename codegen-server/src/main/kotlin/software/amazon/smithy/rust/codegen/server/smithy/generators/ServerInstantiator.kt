/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.generators.Instantiator

/**
 * Server enums do not have an `Unknown` variant like client enums do, so constructing an enum from
 * a string is a fallible operation (hence `try_from`). It's ok to panic here if construction fails,
 * since this is only used in protocol tests.
 */
private fun enumFromStringFn(enumSymbol: Symbol, data: String): Writable = writable {
    rust(
        """#T::try_from($data).expect("This is used in tests ONLY")""",
        enumSymbol,
    )
}

fun serverInstantiator(codegenContext: CodegenContext) =
    Instantiator(
        codegenContext.symbolProvider,
        codegenContext.model,
        codegenContext.runtimeConfig,
        ::enumFromStringFn,
        defaultsForRequiredFields = true,
    )
