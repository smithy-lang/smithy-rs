/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.rust.codegen.rustlang.Feature
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.smithy.customizations.AllowLintsGenerator
import software.amazon.smithy.rust.codegen.smithy.customizations.CrateVersionGenerator
import software.amazon.smithy.rust.codegen.smithy.customizations.SmithyTypesPubUseGenerator
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization

/**
 * A set of customizations that are included in all protocols.
 *
 * This exists as a convenient place to gather these modifications, these are not true customizations.
 *
 * See [RequiredCustomizations] from the `rust-codegen` subproject for the client version of this decorator.
 */
class ServerRequiredCustomizations : RustCodegenDecorator<ServerCodegenContext> {
    override val name: String = "ServerRequired"
    override val order: Byte = -1

    override fun libRsCustomizations(
        codegenContext: ServerCodegenContext,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> =
        baseCustomizations + CrateVersionGenerator() + SmithyTypesPubUseGenerator(codegenContext.runtimeConfig) + AllowLintsGenerator()

    override fun extras(codegenContext: ServerCodegenContext, rustCrate: RustCrate) {
        // Add rt-tokio feature for `ByteStream::from_path`
        rustCrate.mergeFeature(Feature("rt-tokio", true, listOf("aws-smithy-http/rt-tokio")))
    }
}
