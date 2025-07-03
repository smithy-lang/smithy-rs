/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.containerDocs
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.isNotEmpty
import software.amazon.smithy.rust.codegen.core.rustlang.rawRust
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.smithy.CoreRustSettings
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.util.getTrait

sealed class ModuleDocSection {
    data class ServiceDocs(val documentationTraitValue: String?) : ModuleDocSection()

    object CrateOrganization : ModuleDocSection()

    object Examples : ModuleDocSection()
    object AWSSdkUnstable : ModuleDocSection()
}

sealed class LibRsSection(name: String) : Section(name) {
    object Attributes : LibRsSection("Attributes")

    data class ModuleDoc(val subsection: ModuleDocSection) : LibRsSection("ModuleDoc")

    data class Body(val model: Model) : LibRsSection("Body")
}

typealias LibRsCustomization = NamedCustomization<LibRsSection>

class LibRsGenerator(
    private val settings: CoreRustSettings,
    private val model: Model,
    private val customizations: List<LibRsCustomization>,
    private val requireDocs: Boolean,
) {
    private fun docSection(section: ModuleDocSection): List<Writable> =
        customizations
            .map { customization -> customization.section(LibRsSection.ModuleDoc(section)) }
            .filter { it.isNotEmpty() }

    fun render(writer: RustWriter) {
        writer.first {
            customizations.forEach { it.section(LibRsSection.Attributes)(this) }
            rust("##![forbid(unsafe_code)]")
            if (requireDocs) {
                rust("##![warn(missing_docs)]")
            }
            rawRust("#![cfg_attr(docsrs, feature(doc_auto_cfg))]")

            // Allow for overriding the default service docs via customization
            val defaultServiceDocs = settings.getService(model).getTrait<DocumentationTrait>()?.value
            val serviceDocs = docSection(ModuleDocSection.ServiceDocs(defaultServiceDocs))
            if (serviceDocs.isNotEmpty()) {
                serviceDocs.forEach { writeTo ->
                    writeTo(this)
                }
            } else {
                containerDocs(escape(defaultServiceDocs ?: settings.moduleName))
            }

            // Crate Organization
            docSection(ModuleDocSection.CrateOrganization).also { docs ->
                if (docs.isNotEmpty()) {
                    containerDocs("\n## Crate Organization")
                    docs.forEach { writeTo ->
                        writeTo(this)
                    }
                }
            }

            docSection(ModuleDocSection.AWSSdkUnstable).also { docs ->
                if (docs.isNotEmpty()) {
                    containerDocs("\n## Enabling Unstable Features")
                    docs.forEach { writeTo ->
                        writeTo(this)
                    }
                }
            }

            // Examples
            docSection(ModuleDocSection.Examples).also { docs ->
                if (docs.isNotEmpty() || settings.examplesUri != null) {
                    containerDocs("\n## Examples")
                    docs.forEach { writeTo ->
                        writeTo(this)
                    }

                    // TODO(https://github.com/smithy-lang/smithy-rs/issues/69): Generate a basic example for all crates (eg. select first operation and render an example of usage)
                    settings.examplesUri?.also { uri ->
                        containerDocs("Examples can be found [here]($uri).")
                    }
                }
            }

            // TODO(docs): Automated feature documentation
        }
        customizations.forEach { it.section(LibRsSection.Body(model))(writer) }
    }
}
