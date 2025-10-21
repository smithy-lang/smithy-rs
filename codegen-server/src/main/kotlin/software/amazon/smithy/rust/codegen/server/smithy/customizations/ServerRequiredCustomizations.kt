/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.model.Model
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customizations.AllowLintsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customizations.CrateVersionCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customizations.hasBlobs
import software.amazon.smithy.rust.codegen.core.smithy.customizations.hasDateTimes
import software.amazon.smithy.rust.codegen.core.smithy.customizations.hasStreamingOperations
import software.amazon.smithy.rust.codegen.core.smithy.customizations.pubUseSmithyPrimitives
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator

/**
 * A set of customizations that are included in all protocols.
 *
 * This exists as a convenient place to gather these modifications, these are not true customizations.
 *
 * See [RequiredCustomizations] from the `codegen-client` subproject for the client version of this decorator.
 */
class ServerRequiredCustomizations : ServerCodegenDecorator {
    override val name: String = "ServerRequired"
    override val order: Byte = -1

    override fun libRsCustomizations(
        codegenContext: ServerCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> = baseCustomizations + AllowLintsCustomization()

    override fun extras(
        codegenContext: ServerCodegenContext,
        rustCrate: RustCrate,
    ) {
        val rc = codegenContext.runtimeConfig
        val httpDeps = codegenContext.httpDependencies()
        val smithyHttpServerCrate = httpDeps.smithyHttpServer.name

        // Determine the correct http-body feature based on http-1x configuration
        val httpBodyFeature = if (codegenContext.isHttp1()) "http-body-1-x" else "http-body-0-4-x"

        // Add rt-tokio feature for `ByteStream::from_path`
        // Note: pubUseSmithyPrimitives may also add this feature with http-body-1-x hardcoded,
        // but this will override it with the correct feature for our HTTP version
        rustCrate.mergeFeature(
            Feature(
                "rt-tokio",
                true,
                listOf("aws-smithy-types/rt-tokio", "aws-smithy-types/$httpBodyFeature"),
            ),
        )

        rustCrate.mergeFeature(
            Feature(
                "aws-lambda",
                false,
                listOf("$smithyHttpServerCrate/aws-lambda"),
            ),
        )

        rustCrate.mergeFeature(
            Feature(
                "request-id",
                true,
                listOf("$smithyHttpServerCrate/request-id"),
            ),
        )

        rustCrate.withModule(ServerRustModule.Types) {
            if (codegenContext.isHttp1()) {
                pubUseSmithyPrimitives(codegenContext, codegenContext.model, rustCrate)(this)
            } else {
                pubUseSmithyPrimitivesHttp0x(codegenContext, codegenContext.model, rustCrate)(this)
            }

            rustTemplate(
                """
                pub use #{DisplayErrorContext};
                """,
                "Response" to httpDeps.smithyHttpModule().resolve("operation::Response"),
                "DisplayErrorContext" to httpDeps.smithyTypesModule().resolve("error::display::DisplayErrorContext"),
            )
        }

        rustCrate.withModule(ServerRustModule.root) {
            CrateVersionCustomization.extras(rustCrate, ServerRustModule.root)
        }
    }

    /*
    This is almost the same as the core's `pubUseSmithyPrimitivesHttp0x`. The difference is that
    it uses http0x crates in case http-1x flag is false.
     */
    fun pubUseSmithyPrimitivesHttp0x(
        codegenContext: CodegenContext,
        model: Model,
        rustCrate: RustCrate,
    ): Writable =
        writable {
            val rc = codegenContext.runtimeConfig
            if (hasBlobs(model)) {
                rustTemplate("pub use #{Blob};", "Blob" to RuntimeType.blob(rc))
            }
            if (hasDateTimes(model)) {
                rustTemplate(
                    """
                pub use #{DateTime};
                pub use #{Format} as DateTimeFormat;
                """,
                    "DateTime" to RuntimeType.dateTime(rc),
                    "Format" to RuntimeType.format(rc),
                )
            }
            if (hasStreamingOperations(model, codegenContext.serviceShape)) {
                rustCrate.mergeFeature(
                    Feature(
                        "rt-tokio",
                        true,
                        listOf("aws-smithy-types/rt-tokio", "aws-smithy-types/http-body-0-x"),
                    ),
                )
                rustTemplate(
                    """
                pub use #{ByteStream};
                pub use #{AggregatedBytes};
                pub use #{Error} as ByteStreamError;
                ##[cfg(feature = "rt-tokio")]
                pub use #{FsBuilder};
                ##[cfg(feature = "rt-tokio")]
                pub use #{Length};
                pub use #{SdkBody};
                """,
                    "ByteStream" to RuntimeType.smithyTypes(rc).resolve("byte_stream::ByteStream"),
                    "AggregatedBytes" to RuntimeType.smithyTypes(rc).resolve("byte_stream::AggregatedBytes"),
                    "Error" to RuntimeType.smithyTypes(rc).resolve("byte_stream::error::Error"),
                    "FsBuilder" to RuntimeType.smithyTypes(rc).resolve("byte_stream::FsBuilder"),
                    "Length" to RuntimeType.smithyTypes(rc).resolve("byte_stream::Length"),
                    "SdkBody" to RuntimeType.smithyTypes(rc).resolve("body::SdkBody"),
                )
            }
        }
}
