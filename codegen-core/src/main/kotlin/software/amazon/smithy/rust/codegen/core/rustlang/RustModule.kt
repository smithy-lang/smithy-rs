/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.rustlang

data class RustModule(val name: String, val rustMetadata: RustMetadata, val documentation: String?) {
    fun render(writer: RustWriter) {
        documentation?.let { docs -> writer.docs(docs) }
        rustMetadata.render(writer)
        writer.write("mod $name;")
    }

    companion object {
        fun default(name: String, visibility: Visibility, documentation: String? = null): RustModule {
            return RustModule(name, RustMetadata(visibility = visibility), documentation)
        }

        fun public(name: String, documentation: String? = null): RustModule =
            default(name, visibility = Visibility.PUBLIC, documentation = documentation)

        fun private(name: String, documentation: String? = null): RustModule =
            default(name, visibility = Visibility.PRIVATE, documentation = documentation)

        /* Common modules used across client, server and tests */
        val Config = public("config", documentation = "Configuration for the service.")
        val Error = public("error", documentation = "All error types that operations can return.")
        val Model = public("model", documentation = "Data structures used by operation inputs/outputs.")
        val Input = public("input", documentation = "Input structures for operations.")
        val Output = public("output", documentation = "Output structures for operations.")

        /**
         * Helper method to generate the `operation` Rust module.
         * Its visibility depends on the generation context (client or server).
         */
        fun operation(visibility: Visibility): RustModule =
            default("operation", visibility = visibility, documentation = "All operations that this crate can perform.")
    }
}
