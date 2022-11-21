/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.rustlang

/**
 * RustModule system.
 *
 * RustModules are idempotent, BUT, you must take care to always use the same module (matching docs, visibility, etc.):
 * - There is no guarantee _which_ module will be rendered.
 */
sealed class RustModule {

    /** lib.rs */
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
                "Module names CANNOT contain `::`—modules must be nested with parent (name was: `$name`)"
            }

            duplicateModuleWarningSystem[fullyQualifiedPath()]?.also { preexistingModule ->
                check(this == preexistingModule) {
                    "Duplicate modules with differing properties were created! This will lead to non-deterministic behavior." +
                        "\n Previous module: $preexistingModule." +
                        "\n New module: $this"
                }
            }
            duplicateModuleWarningSystem[fullyQualifiedPath()] = this
        }
    }

    companion object {
        // used to ensure we never create accidentally discard docs / create variable visibility
        private var duplicateModuleWarningSystem: MutableMap<String, LeafModule> = mutableMapOf()

        /** Creates a new module with the specified visibility */
        fun new(
            name: String,
            visibility: Visibility,
            documentation: String? = null,
            parent: RustModule = LibRs,
        ): LeafModule {
            return LeafModule(name, RustMetadata(visibility = visibility), documentation, parent = parent)
        }

        fun pubcrate(name: String, documentation: String? = null, parent: RustModule = LibRs): LeafModule =
            new(name, visibility = Visibility.PUBCRATE, documentation, parent)

        /** Creates a new public module */
        fun public(name: String, documentation: String? = null, parent: RustModule = LibRs): LeafModule =
            new(name, visibility = Visibility.PUBLIC, documentation = documentation, parent = parent)

        /** Creates a new private module */
        fun private(name: String, documentation: String? = null, parent: RustModule = LibRs): LeafModule =
            new(name, visibility = Visibility.PRIVATE, documentation = documentation, parent = parent)

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
            new(
                "operation",
                visibility = visibility,
                documentation = "All operations that this crate can perform.",
            )
    }

    /**
     * Fully qualified path to this module, e.g. `crate::grandparent::parent::child`
     */
    fun fullyQualifiedPath(): String = when (this) {
        is LibRs -> "crate"
        is LeafModule -> parent.fullyQualifiedPath() + "::" + name
    }

    /**
     * The file this module is homed in, e.g. `src/grandparent/parent/child.rs`
     */
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
}
