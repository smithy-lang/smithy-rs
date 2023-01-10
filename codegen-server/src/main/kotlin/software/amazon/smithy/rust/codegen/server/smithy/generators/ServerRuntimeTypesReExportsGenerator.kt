/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerRuntimeType

class ServerRuntimeTypesReExportsGenerator(
    codegenContext: CodegenContext,
) {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope = arrayOf(
        "Router" to ServerRuntimeType.router(runtimeConfig),
        "SmithyHttpServer" to ServerCargoDependency.smithyHttpServer(runtimeConfig).toType(),
    )

    fun render(writer: RustWriter) {
        writer.rustTemplate(
            """
            pub mod body {
                pub use #{SmithyHttpServer}::body::BoxBody;
            }
            pub mod operation {
                pub use #{SmithyHttpServer}::operation::OperationShape;
                pub use #{SmithyHttpServer}::operation::Operation;
            }
            pub mod plugin {
                pub use #{SmithyHttpServer}::plugin::Plugin;
                pub use #{SmithyHttpServer}::plugin::PluginPipeline;
                pub use #{SmithyHttpServer}::plugin::PluginStack;
            }
            pub mod request {
                pub use #{SmithyHttpServer}::request::FromParts;
            }
            pub mod response {
                pub use #{SmithyHttpServer}::response::IntoResponse;
            }
            pub mod routing {
                pub use #{SmithyHttpServer}::routing::IntoMakeService;
                pub use #{SmithyHttpServer}::routing::IntoMakeServiceWithConnectInfo;
                pub use #{SmithyHttpServer}::routing::Router;

                ##[cfg(feature = "aws-lambda")]
                pub use #{SmithyHttpServer}::routing::LambdaHandler;
            }

            pub use #{SmithyHttpServer}::instrumentation;
            pub use #{SmithyHttpServer}::proto;

            pub use #{SmithyHttpServer}::Extension;
            """,
            *codegenScope,
        )
    }
}
