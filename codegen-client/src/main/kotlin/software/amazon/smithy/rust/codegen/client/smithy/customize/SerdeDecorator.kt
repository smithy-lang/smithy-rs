/*
* Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
* SPDX-License-Identifier: Apache-2.0
*/

package software.amazon.smithy.rust.codegen.client.smithy.customize

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.ModuleDocSection

/**
 * This class,
 * - Adds serde as a dependency
 *
 */
class SerdeDecorator : ClientCodegenDecorator {
    override val name: String = "SerdeDecorator"
    override val order: Byte = -1

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        fun feature(featureName: String): Feature {
            return Feature(featureName, false, listOf("aws-smithy-types/$featureName"))
        }
        rustCrate.mergeFeature(feature("serde-serialize"))
        rustCrate.mergeFeature(feature("serde-deserialize"))
    }

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> = baseCustomizations + SerdeDocGenerator(codegenContext)
}

class SerdeDocGenerator(private val codegenContext: ClientCodegenContext) : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable {
        return if (section is LibRsSection.ModuleDoc && section.subsection is ModuleDocSection.UnstableFeature) {
            writable {
                """
                ## How to enable `Serialize` and `Deserialize`
                This data type implements `Serialize` and `Deserialize` traits from the popular serde crate,
                but those traits are behind feature gate.

                As they increase it's compile time dramatically, you should not turn them on unless it's necessary.
                """.trimIndent()
            }
        } else {
            emptySection
        }
    }
}
