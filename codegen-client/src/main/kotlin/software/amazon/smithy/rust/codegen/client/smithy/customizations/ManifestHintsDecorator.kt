/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.smithy.generators.ManifestCustomizations

/**
 * See https://blog.rust-lang.org/inside-rust/2025/07/15/call-for-testing-hint-mostly-unused/ for more information
 */
data class ManifestHintsSettings(
    val mostlyUnused: Boolean? = null,
)

fun ManifestHintsSettings.asMap(): Map<String, Any> {
    val inner =
        listOfNotNull(
            mostlyUnused?.let { "mostly-unused" to it },
        ).toMap()
    return mapOf("hints" to inner)
}

/**
 * Write compilation hints metdata settings into Cargo.toml
 *
 * # Notes
 * This decorator is not used by default, code generators must manually configure and include it in their builds.
 */
class ManifestHintsDecorator(private val manifestHintsSettings: ManifestHintsSettings) : ClientCodegenDecorator {
    override val name: String = "ManifestHintsDecorator"
    override val order: Byte = 0

    override fun crateManifestCustomizations(codegenContext: ClientCodegenContext): ManifestCustomizations {
        if (codegenContext.settings.hintMostlyUnusedList.contains(codegenContext.settings.moduleName)) {
            return manifestHintsSettings.asMap()
        } else {
            return emptyMap()
        }
    }
}
