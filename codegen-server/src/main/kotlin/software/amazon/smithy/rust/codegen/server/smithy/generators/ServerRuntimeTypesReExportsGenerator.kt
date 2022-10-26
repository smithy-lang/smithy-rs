/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.asType
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerRuntimeType

class ServerRuntimeTypesReExportsGenerator(
    codegenContext: CodegenContext,
) {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope = arrayOf(
        "Router" to ServerRuntimeType.Router(runtimeConfig),
        "SmithyHttpServer" to ServerCargoDependency.SmithyHttpServer(runtimeConfig).asType(),
    )

    fun render(writer: RustWriter) {
        writer.rustTemplate(
            """
            pub use #{SmithyHttpServer}::Extension;
            pub use #{Router};
            pub use #{SmithyHttpServer}::operation::OperationShape;
            pub use #{SmithyHttpServer}::operation::Operation;

            ##[doc(hidden)]
            pub use #{SmithyHttpServer}::instrumentation;
            ##[doc(hidden)]
            pub use #{SmithyHttpServer}::proto;
            """,
            *codegenScope,
        )
    }
}
