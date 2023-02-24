/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.smithy.generators.ManifestCustomizations

/**
 * See https://docs.rs/about/metadata for more information
 */
data class DocsRsMetadataSettings(
    val features: List<String>? = null,
    val allFeatures: Boolean? = null,
    val noDefaultFeatures: Boolean? = null,
    val defaultTarget: String? = null,
    val targets: List<String>? = null,
    val rustcArgs: List<String>? = null,
    val rustdocArgs: List<String>? = null,
    val cargoArgs: List<String>? = null,
    /** Any custom key-value pairs to be inserted into the docsrs metadata */
    val custom: HashMap<String, Any> = HashMap(),
)

fun DocsRsMetadataSettings.asMap(): Map<String, Any> {
    val inner = listOfNotNull(
        features?.let { "features" to it },
        allFeatures?.let { "all-features" to it },
        noDefaultFeatures?.let { "no-default-features" to it },
        defaultTarget?.let { "no-default-target" to it },
        targets?.let { "targets" to it },
        rustcArgs?.let { "rustc-args" to it },
        rustdocArgs?.let { "rustdoc-args" to it },
        cargoArgs?.let { "cargo-args" to it },
    ).toMap() + custom
    return mapOf("package" to mapOf("metadata" to mapOf("docs" to mapOf("rs" to inner))))
}

/**
 * Write docs.rs metdata settings into Cargo.toml
 *
 * docs.rs can be configured via data set at `[package.metadata.docs.rs]`. This decorator will write [DocsRsMetadataSettings]
 * into the appropriate location.
 *
 * # Notes
 * This decorator is not used by default, code generators must manually configure and include it in their builds.
 */
class DocsRsMetadataDecorator(private val docsRsMetadataSettings: DocsRsMetadataSettings) : ClientCodegenDecorator {
    override val name: String = "DocsRsMetadataDecorator"
    override val order: Byte = 0

    override fun crateManifestCustomizations(codegenContext: ClientCodegenContext): ManifestCustomizations {
        return docsRsMetadataSettings.asMap()
    }
}
