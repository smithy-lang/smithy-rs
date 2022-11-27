/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.containerDocs
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.isEmpty
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.smithy.CoreRustSettings
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedSectionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.util.getTrait

sealed class LibRsSection(name: String) : Section(name) {
    object Attributes : LibRsSection("Attributes")
    data class ModuleDocumentation(val subsection: String) : LibRsSection("ModuleDocumentation")
    data class Body(val model: Model) : LibRsSection("Body")
    companion object {
        val Examples = "Examples"
        val CrateOrganization = "CrateOrganization"
    }
}

typealias LibRsCustomization = NamedSectionGenerator<LibRsSection>

class LibRsGenerator(
    private val settings: CoreRustSettings,
    private val model: Model,
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
            containerDocs(escape(libraryDocs))
            val crateLayout = customizations.map { it.section(LibRsSection.ModuleDocumentation(LibRsSection.CrateOrganization)) }.filter { !it.isEmpty() }
            if (crateLayout.isNotEmpty()) {
                containerDocs("\n## Crate Organization")
                crateLayout.forEach { it(this) }
            }

            val examples = customizations.map { it.section(LibRsSection.ModuleDocumentation(LibRsSection.Examples)) }
                .filter { section -> !section.isEmpty() }
            if (examples.isNotEmpty() || settings.examplesUri != null) {
                containerDocs("\n## Examples")
                examples.forEach { it(this) }

                // TODO(https://github.com/awslabs/smithy-rs/issues/69): Generate a basic example for all crates (eg. select first operation and render an example of usage)
                settings.examplesUri?.also { uri ->
                    containerDocs("Examples can be found [here]($uri).")
                }
            }

            // TODO(docs): Automated feature documentation
        }
        customizations.forEach { it.section(LibRsSection.Body(model))(writer) }
    }
}
