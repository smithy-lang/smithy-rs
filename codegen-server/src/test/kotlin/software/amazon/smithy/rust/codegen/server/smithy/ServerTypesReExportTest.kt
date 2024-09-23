/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
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
        @http(uri: "/sample", method: "GET")
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

                // Ensure all types that were exported before version 0.64 and used
                // under the `{generated_sdk_crate_name}::server` namespace remain available.
                // Additionally, include all types requested by customers.
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
                        "scope",
                    ).generateUseStatements("crate::server"),
                )

                unitTest(
                    "request_id_reexports",
                    additionalAttributes = listOf(Attribute.featureGate("request-id")),
                ) {
                    rustTemplate(
                        """
                        ##[allow(unused_imports)] use crate::server::request::request_id::ServerRequestId;
                        """,
                    )
                }

                unitTest(
                    "aws_lambda_reexports",
                    additionalAttributes = listOf(Attribute.featureGate("aws-lambda")),
                ) {
                    rustTemplate(
                        """
                        ##[allow(unused_imports)] use crate::server::{request::lambda::Context, routing::LambdaHandler};
                        """,
                    )
                }
            }
        }
    }
}
