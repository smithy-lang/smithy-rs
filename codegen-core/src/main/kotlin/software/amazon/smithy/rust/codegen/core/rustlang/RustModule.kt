/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.rustlang

sealed class RustModule {
    companion object {
        /** Creates a new module with the specified visibility */
        fun newModule(
            name: String,
            visibility: Visibility,
            documentation: String? = null,
            parent: RustModule = LibRs,
        ): LeafModule {
            return LeafModule(name, RustMetadata(visibility = visibility), documentation, parent = parent)
        }

        fun pubcrate(name: String, documentation: String? = null, parent: RustModule = LibRs): LeafModule =
            newModule(name, visibility = Visibility.PUBCRATE, documentation, parent)

        /** Creates a new public module */
        fun public(name: String, documentation: String? = null, parent: RustModule = LibRs): LeafModule =
            newModule(name, visibility = Visibility.PUBLIC, documentation = documentation, parent = parent)

        /** Creates a new private module */
        fun private(name: String, documentation: String? = null, parent: RustModule = LibRs): LeafModule =
            newModule(name, visibility = Visibility.PRIVATE, documentation = documentation)

        /* Common modules used across client, server and tests */
        val Config = public("config", documentation = "Configuration for the service.")
        val Error = public("error", documentation = "All error types that operations can return.")
        val Model = public("model", documentation = "Data structures used by operation inputs/outputs.")
        val Input = public("input", documentation = "Input structures for operations.")
        val Output = public("output", documentation = "Output structures for operations.")
        val Types = public("types", documentation = "Data primitives referenced by other data types.")

        /**
         * Helper method to generate the `operation` Rust module.
         * Its visibility depends on the generation context (client or server).
         */
        fun operation(visibility: Visibility): RustModule =
            newModule(
                "operation",
                visibility = visibility,
                documentation = "All operations that this crate can perform.",
            )
    }

    fun fullyQualifiedPath(): String = when (this) {
        is LibRs -> "crate"
        is LeafModule -> parent.fullyQualifiedPath() + "::" + name
    }

    fun definitionFile(): String = when (this) {
        is LibRs -> "src/lib.rs"
        is LeafModule -> {
            val path = fullyQualifiedPath().split("::").drop(1).joinToString("/")
            "src/$path.rs"
        }
    }

    /**
     * Renders the usage statement, approximately:
     * ```rust
     * /// My docs
     * pub mod my_module_name
     * ```
     */
    fun renderModStatement(writer: RustWriter) {
        when (this) {
            is LeafModule -> {
                documentation?.let { docs -> writer.docs(docs) }
                rustMetadata.render(writer)
                writer.write("mod $name;")
            }

            else -> {}
        }
    }

    object LibRs : RustModule()

    /**
     * LeafModule
     *
     * A LeafModule is _all_ modules that are not `lib.rs`. To create a nested leaf module, set `parent` to a module
     * _other_ than `LibRs`.
     *
     * To avoid infinite loops, avoid setting parent to itself ;-)
     */
    data class LeafModule(
        val name: String,
        val rustMetadata: RustMetadata,
        val documentation: String? = null,
        val parent: RustModule = LibRs,
    ) : RustModule() {
        init {
            check(!name.contains("::")) {
                "Modules CANNOT contain `::`—modules must be nested with parent"
            }
        }
    }
}
