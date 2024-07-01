/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency

class ServerRuntimeTypesReExportsGenerator(
    codegenContext: CodegenContext,
) {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
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
            }
            pub mod plugin {
                pub use #{SmithyHttpServer}::plugin::HttpPlugins;
                pub use #{SmithyHttpServer}::plugin::ModelPlugins;
                pub use #{SmithyHttpServer}::plugin::HttpMarker;
                pub use #{SmithyHttpServer}::plugin::ModelMarker;
                pub use #{SmithyHttpServer}::plugin::Plugin;
                pub use #{SmithyHttpServer}::plugin::PluginStack;
            }
            pub mod request {
                pub use #{SmithyHttpServer}::request::FromParts;

                ##[cfg(feature = "aws-lambda")]
                pub mod lambda {
                    pub use #{SmithyHttpServer}::request::lambda::Context;
                }
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
            pub use #{SmithyHttpServer}::protocol;

            pub use #{SmithyHttpServer}::Extension;
            """,
            *codegenScope,
        )
    }
}
