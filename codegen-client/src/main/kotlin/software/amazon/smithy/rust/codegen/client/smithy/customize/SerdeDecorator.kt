/*
* Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
* SPDX-License-Identifier: Apache-2.0
*/

package software.amazon.smithy.rust.codegen.client.smithy.customize

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.containerDocs
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.ModuleDocSection

/**
 * Decorator that adds the `serde-serialize` and `serde-deserialize` features.
 */
class SerdeDecorator : ClientCodegenDecorator {
    override val name: String = "SerdeDecorator"
    override val order: Byte = 5

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
    ): List<LibRsCustomization> {
        return baseCustomizations + SerdeDocGenerator(codegenContext)
    }
}

class SerdeDocGenerator(private val codegenContext: ClientCodegenContext) : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable {
        return when (section) {
            is LibRsSection.ModuleDoc ->
                if (section.subsection is ModuleDocSection.AWSSdkUnstable) {
                    serdeInfoText()
                } else {
                    emptySection
                }

            else -> emptySection
        }
    }

    private fun serdeInfoText(): Writable {
        return writable {
            containerDocs(
                """
                ## How to enable `Serialize` and `Deserialize`

                This data type implements `Serialize` and `Deserialize` traits from the popular serde crate, but those traits are behind feature gate.

                As they increase it's compile time dramatically, you should not turn them on unless it's necessary.
                Implementation of `serde` traits in AWS SDK for Rust is still unstable, and implementation may change anytime in future.

                To enable traits, you must pass `aws_sdk_unstable` to RUSTFLAGS and enable `serde-serialize` or `serde-deserialize` feature.

                e.g.
                ```bash,no_run
                export RUSTFLAGS="--cfg aws_sdk_unstable"
                cargo build --features serde-serialize serde-deserialize
                ```

                If you enable `serde-serialize` and/or `serde-deserialize` without `RUSTFLAGS="--cfg aws_sdk_unstable"`,
                compilation will fail with warning.

                """.trimIndent(),
            )
        }
    }
}
