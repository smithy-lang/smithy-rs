/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext

class ServerRuntimeTypesReExportsGenerator(
    codegenContext: CodegenContext,
) {
    private val serverCodegenContext = codegenContext as software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
    private val codegenScope =
        arrayOf(
            "SmithyHttpServer" to serverCodegenContext.httpDependencies().smithyHttpServer.toType(),
        )

    fun render(writer: RustWriter) {
        writer.rustTemplate(
            """
            // Re-export all types from the `aws-smithy-http-server` crate.
            pub use #{SmithyHttpServer}::*;
            """,
            *codegenScope,
        )
    }
}
