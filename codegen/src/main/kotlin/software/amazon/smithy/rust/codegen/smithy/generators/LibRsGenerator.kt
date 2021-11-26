/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.escape
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.superDocs
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RustSettings
import software.amazon.smithy.rust.codegen.smithy.customize.NamedSectionGenerator
import software.amazon.smithy.rust.codegen.smithy.customize.Section
import software.amazon.smithy.rust.codegen.util.getTrait

sealed class LibRsSection(name: String) : Section(name) {
    object Attributes : LibRsSection("Attributes")
    data class ModuleDocumentation(val subsection: String) : LibRsSection("ModuleDocumentation")
    object Body : LibRsSection("Body")
}

typealias LibRsCustomization = NamedSectionGenerator<LibRsSection>

class LibRsGenerator(
    private val settings: RustSettings,
    private val model: Model,
    private val modules: List<RustModule>,
    private val customizations: List<LibRsCustomization>,
    private val requireDocs: Boolean,
) {
    fun render(writer: RustWriter) {
        writer.first {
            customizations.forEach { it.section(LibRsSection.Attributes)(this) }
            if (requireDocs) {
                rust("##![warn(missing_docs)]")
            }

            val libraryDocs = settings.getService(model).getTrait<DocumentationTrait>()?.value ?: settings.moduleName
            superDocs(escape(libraryDocs))
            // TODO: replace "service" below with the title trait
            superDocs(
                """
                ## Crate Organization

                The entry point for must customers will be [`Client`]. [`Client`] exposes one method for each API offered
                by the service.

                Some APIs require complex or nested arguments. These exist in [`model`].

                Lastly, errors that can be returned by the service are contained within [`error`]. [`Error`] defines a meta
                error encompassing all possible errors that can be returned by the service.

                The other modules within this crate and not required for normal usage."""
            )

            val examples = customizations.map { it.section(LibRsSection.ModuleDocumentation("Examples")) }
                .filter { it != writable { } }
            if (examples.isNotEmpty() || settings.examplesUri != null) {
                superDocs("## Examples")
                examples.forEach { it(this) }

                // TODO: Render a basic example for all crates (eg. select first operation and render an example of usage)
                settings.examplesUri?.also { uri ->
                    superDocs("Examples can be found [here]($uri).")
                }
            }

            // TODO: Automated feature documentation
        }
        modules.forEach { it.render(writer) }
        customizations.forEach { it.section(LibRsSection.Body)(writer) }
    }
}
