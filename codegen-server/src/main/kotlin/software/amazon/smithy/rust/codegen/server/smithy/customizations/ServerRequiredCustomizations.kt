/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customizations.AllowLintsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customizations.CrateVersionCustomization
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
    ): List<LibRsCustomization> =
        // TODO(https://github.com/smithy-lang/smithy-rs/issues/4366) Remove additionalClippyLints once the issue is resolved
        baseCustomizations + AllowLintsCustomization(additionalClippyLints = listOf("uninlined_format_args"))

    override fun extras(
        codegenContext: ServerCodegenContext,
        rustCrate: RustCrate,
    ) {
        val rc = codegenContext.runtimeConfig

        // Add rt-tokio feature for `ByteStream::from_path`
        rustCrate.mergeFeature(
            Feature(
                "rt-tokio",
                true,
                listOf("aws-smithy-types/rt-tokio"),
            ),
        )

        rustCrate.mergeFeature(
            Feature(
                "aws-lambda",
                false,
                listOf("aws-smithy-http-server/aws-lambda"),
            ),
        )

        rustCrate.mergeFeature(
            Feature(
                "request-id",
                true,
                listOf("aws-smithy-http-server/request-id"),
            ),
        )

        rustCrate.withModule(ServerRustModule.Types) {
            pubUseSmithyPrimitives(codegenContext, codegenContext.model, rustCrate)(this)
            rustTemplate(
                """
                pub use #{DisplayErrorContext};
                """,
                "Response" to RuntimeType.smithyHttp(rc).resolve("operation::Response"),
                "DisplayErrorContext" to RuntimeType.smithyTypes(rc).resolve("error::display::DisplayErrorContext"),
            )
        }

        rustCrate.withModule(ServerRustModule.root) {
            CrateVersionCustomization.extras(rustCrate, ServerRustModule.root)
        }
    }
}
