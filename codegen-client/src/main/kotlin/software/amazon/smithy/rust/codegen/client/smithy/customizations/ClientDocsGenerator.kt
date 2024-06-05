/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.traits.TitleTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.containerDocs
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.DirectedWalker
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.ModuleDocSection
import software.amazon.smithy.rust.codegen.core.util.getTrait

class ClientDocsGenerator(private val codegenContext: ClientCodegenContext) : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable {
        return when (section) {
            is LibRsSection.ModuleDoc ->
                if (section.subsection is ModuleDocSection.CrateOrganization) {
                    crateLayout()
                } else {
                    emptySection
                }
            else -> emptySection
        }
    }

    private fun crateLayout(): Writable =
        writable {
            val hasTypesModule =
                DirectedWalker(codegenContext.model).walkShapes(codegenContext.serviceShape)
                    .any {
                        try {
                            codegenContext.symbolProvider.moduleForShape(it).name == ClientRustModule.types.name
                        } catch (ex: RuntimeException) {
                            // The shape should not be rendered in any module.
                            false
                        }
                    }
            val typesModuleSentence =
                if (hasTypesModule) {
                    "These structs and enums live in [`types`](crate::types). "
                } else {
                    ""
                }
            val serviceName = codegenContext.serviceShape.getTrait<TitleTrait>()?.value ?: "the service"
            containerDocs(
                """
                The entry point for most customers will be [`Client`], which exposes one method for each API
                offered by $serviceName. The return value of each of these methods is a "fluent builder",
                where the different inputs for that API are added by builder-style function call chaining,
                followed by calling `send()` to get a [`Future`](std::future::Future) that will result in
                either a successful output or a [`SdkError`](crate::error::SdkError).

                Some of these API inputs may be structs or enums to provide more complex structured information.
                ${typesModuleSentence}There are some simpler types for
                representing data such as date times or binary blobs that live in [`primitives`](crate::primitives).

                All types required to configure a client via the [`Config`](crate::Config) struct live
                in [`config`](crate::config).

                The [`operation`](crate::operation) module has a submodule for every API, and in each submodule
                is the input, output, and error type for that API, as well as builders to construct each of those.

                There is a top-level [`Error`](crate::Error) type that encompasses all the errors that the
                client can return. Any other error type can be converted to this `Error` type via the
                [`From`](std::convert::From) trait.

                The other modules within this crate are not required for normal usage.
                """.trimEnd(),
            )
        }
}
