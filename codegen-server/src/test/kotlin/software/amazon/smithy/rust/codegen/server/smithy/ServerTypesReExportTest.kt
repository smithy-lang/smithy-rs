package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class ServerTypesReExportTest {
    private val sampleModel =
        """
        namespace amazon

        use aws.protocols#restJson1

        @restJson1
        service SampleService {
            operations: [SampleOperation]
        }

        @http(uri: "/anOperation", method: "GET")
        operation SampleOperation {
            output := {}
        }
        """.asSmithyModel(smithyVersion = "2")

    @Test
    fun `ensure types are exported from aws-smithy-http-server`() {
        serverIntegrationTest(sampleModel, IntegrationTestParams(service = "amazon#SampleService")) { _, rustCrate ->
            rustCrate.testModule {
                fun Set<String>.generateUseStatements(prefix: String) =
                    this.joinToString(separator = "\n") {
                        "#[allow(unused_imports)] use $prefix::$it;"
                    }

                unitTest(
                    "types_exists_in_server_module",
                    setOf(
                        "extension::{OperationExtensionExt, OperationExtension}",
                        "plugin::Scoped",
                        "routing::{Route, RoutingService}",
                        "body::boxed",
                        "shape_id::ShapeId",
                        "body::BoxBody",
                        "operation::OperationShape",
                        "plugin::HttpPlugins",
                        "plugin::ModelPlugins",
                        "plugin::HttpMarker",
                        "plugin::ModelMarker",
                        "plugin::Plugin",
                        "plugin::PluginStack",
                        "request::{self, FromParts}",
                        "response::IntoResponse",
                        "routing::IntoMakeService",
                        "routing::IntoMakeServiceWithConnectInfo",
                        "routing::Router",
                        "instrumentation",
                        "protocol",
                        "Extension",
                        ).generateUseStatements("crate::server"),
                )
            }
        }
    }
}
