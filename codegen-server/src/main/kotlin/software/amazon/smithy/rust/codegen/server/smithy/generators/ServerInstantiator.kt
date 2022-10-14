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

private fun enumFromStringFn(enumSymbol: Symbol, data: String): Writable = writable {
    rust(
        """#T::try_from($data).expect("This is used in tests ONLY")""",
        enumSymbol,
    )
}

class ServerInstantiator(val codegenContext: CodegenContext) :
    Instantiator(
        codegenContext.symbolProvider,
        codegenContext.model,
        codegenContext.runtimeConfig,
        ::enumFromStringFn,
        defaultsForRequiredFields = true,
    )
